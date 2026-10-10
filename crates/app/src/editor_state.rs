//! Bounded native editor history, initialized once for each document.
use crate::model::Document;
use eframe::egui;
pub const MAX_UNDO_STATES: usize = 16;
type EditorUndoer = egui::util::undoer::Undoer<(egui::text::CCursorRange, String)>;

pub struct CursorHistory {
    history: EditorUndoer,
    cursor: Option<egui::text::CCursorRange>,
    programmatic: bool,
}

pub fn load(ctx: &egui::Context, doc: &mut Document) -> egui::text_edit::TextEditState {
    let id = egui::Id::new(("editor", doc.id));
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    if !doc.undo_initialized {
        state.set_undoer(egui::util::undoer::Undoer::with_settings(
            egui::util::undoer::Settings {
                max_undos: MAX_UNDO_STATES,
                ..Default::default()
            },
        ));
        doc.undo_initialized = true;
        state.clone().store(ctx, id);
    }
    state
}

/// Native egui history includes cursor/selection in its equality test. Navigation
/// can therefore add same-text checkpoints. An editor Undo/Redo shortcut should
/// cross those checkpoints and land on the next text change in one key press.
/// History is cloned only for an explicit shortcut, never during idle frames.
pub fn history_shortcut(ctx: &egui::Context, doc: &mut Document) {
    let id = egui::Id::new(("editor", doc.id));
    if !ctx.memory(|memory| memory.has_focus(id)) {
        return;
    }
    let actions = ctx.input_mut(|input| {
        let mut actions = Vec::new();
        for event in &input.events {
            if let Some(redo) = history_event(event) {
                if actions.len() == MAX_UNDO_STATES {
                    return None;
                }
                actions.push(redo);
            } else if !matches!(event, egui::Event::Key { pressed: false, .. }) {
                // Text, paste, pointer, navigation and other events must keep their
                // exact native ordering relative to Undo/Redo. Pass mixed batches
                // through untouched rather than replaying any editor input here.
                return None;
            }
        }
        if actions.is_empty() {
            return None;
        }
        input.events.retain(|event| history_event(event).is_none());
        Some(actions)
    });
    if let Some(actions) = actions {
        for redo in actions {
            step_history(ctx, doc, redo);
        }
    }
}

/// egui's undo state compares (selection, text), so a focus click or selection
/// move clears redo even when no text changes. Preserve its bounded history only
/// for an explicit cursor interaction, then restore it if the widget made no
/// text edit. There is no second persistent history and no idle/hover cloning.
pub fn before_cursor_interaction(
    ctx: &egui::Context,
    doc: &mut Document,
    programmatic_cursor: bool,
) -> Option<CursorHistory> {
    let id = egui::Id::new(("editor", doc.id));
    let focused = ctx.memory(|memory| memory.has_focus(id));
    let rect = ctx.read_response(id).map(|response| response.rect);
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let cursor_input = ctx.input(|input| {
        input.events.iter().any(|event| match event {
            egui::Event::PointerButton { pos, .. } | egui::Event::Touch { pos, .. } => {
                rect.is_some_and(|rect| rect.contains(*pos))
            }
            egui::Event::PointerMoved(_) => focused && input.pointer.any_down(),
            egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } if focused => {
                matches!(
                    key,
                    egui::Key::ArrowLeft
                        | egui::Key::ArrowRight
                        | egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::Home
                        | egui::Key::End
                ) || (*key == egui::Key::A && modifiers.command)
                    || (mac
                        && modifiers.ctrl
                        && !modifiers.shift
                        && matches!(
                            key,
                            egui::Key::P
                                | egui::Key::N
                                | egui::Key::B
                                | egui::Key::F
                                | egui::Key::A
                                | egui::Key::E
                        ))
            }
            _ => false,
        })
    });
    if !programmatic_cursor && !cursor_input {
        return None;
    }
    let state = load(ctx, doc);
    Some(CursorHistory {
        history: state.undoer(),
        cursor: state.cursor.char_range(),
        programmatic: programmatic_cursor,
    })
}

pub fn after_cursor_interaction(
    ctx: &egui::Context,
    doc: &Document,
    output: &egui::text_edit::TextEditOutput,
    history: Option<CursorHistory>,
) {
    let Some(history) = history else {
        return;
    };
    if output.response.changed() {
        // Text, paste, IME, and native mixed Undo/Redo batches keep their actual
        // event order. A real edit must invalidate redo in the ordinary way.
        return;
    }
    let Some(cursor) = output.state.cursor.char_range() else {
        return;
    };
    if history.cursor == Some(cursor) && !history.programmatic {
        return;
    }
    // Coalesce selection-only points rather than appending one per arrow/drag
    // frame: those points would otherwise evict meaningful text history.
    let history = replace_history_cursor(history.history, cursor, &doc.text);
    let mut state = output.state.clone();
    state.set_undoer(history);
    state.store(ctx, egui::Id::new(("editor", doc.id)));
}

/// The pinned public Undoer API cannot replace its latest selection in place.
/// Rebuild the same bounded native history only at a cursor change. Temporary
/// branches expose past/future through the public Undo/Redo API; no extra
/// history survives this call. Adjacent equal-text points collapse into one.
fn replace_history_cursor(
    mut past: EditorUndoer,
    cursor: egui::text::CCursorRange,
    text: &str,
) -> EditorUndoer {
    let mut future = past.clone();
    // A cursor beyond every possible buffer distinguishes the probe from a
    // real checkpoint. undo(probe) reveals the latest point without popping it.
    // Its synthetic redo entry stays only in the disposable past branch.
    let probe = (
        egui::text::CCursorRange::one(egui::text::CCursor::new(usize::MAX)),
        String::new(),
    );
    let anchor = past.undo(&probe).cloned();
    if anchor.as_ref().is_some_and(|state| {
        state.0 == cursor
            && state.0.primary.prefer_next_row == cursor.primary.prefer_next_row
            && state.0.secondary.prefer_next_row == cursor.secondary.prefer_next_row
            && state.1 == text
    }) {
        return future;
    }
    let mut states = Vec::with_capacity(MAX_UNDO_STATES);
    if let Some(anchor) = &anchor {
        states.push(anchor.clone());
        let mut current = anchor.clone();
        for _ in 0..MAX_UNDO_STATES {
            let Some(previous) = past.undo(&current).cloned() else {
                break;
            };
            current = previous;
            if states.last().is_none_or(|last| last.1 != current.1) {
                states.push(current.clone());
            }
        }
    }
    states.reverse();
    let current = (cursor, text.to_owned());
    if states.last().is_some_and(|last| last.1 == text) {
        *states.last_mut().unwrap() = current;
    } else {
        states.push(current);
    }
    let mut redo_count = 0;
    if let Some(mut current) = anchor.filter(|anchor| anchor.1 == text) {
        // Keep the complete bounded redo chain, not just the next text. Native
        // history can also retain one uncheckpointed typing state beyond its
        // 16 saved points after Undo.
        for _ in 0..MAX_UNDO_STATES {
            let Some(next) = future.redo(&current).cloned() else {
                break;
            };
            current = next;
            if states.last().is_none_or(|last| last.1 != current.1) {
                states.push(current.clone());
                redo_count += 1;
            }
        }
    }
    let mut rebuilt = EditorUndoer::with_settings(egui::util::undoer::Settings {
        max_undos: MAX_UNDO_STATES,
        ..Default::default()
    });
    let extra_typing_state = states.len() > MAX_UNDO_STATES;
    let saved = if extra_typing_state {
        &states[..states.len() - 1]
    } else {
        &states[..]
    };
    for state in saved {
        rebuilt.add_undo(state);
    }
    let mut current = states.last().unwrap().clone();
    if extra_typing_state && redo_count == 0 {
        // Fully redone history may contain 16 checkpoints plus the former
        // typing state. Native undo/redo can append that one extra state without
        // trimming the oldest checkpoint; reproduce that public API behavior.
        if let Some(previous) = rebuilt.undo(&current).cloned() {
            rebuilt.redo(&previous);
        }
    }
    for _ in 0..redo_count {
        // At any partial-Undo split of a 17-state timeline, the first undo
        // captures the final typing state without evicting the oldest point.
        if let Some(previous) = rebuilt.undo(&current).cloned() {
            current = previous;
        }
    }
    rebuilt
}

fn history_event(event: &egui::Event) -> Option<bool> {
    let egui::Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    if (*key == egui::Key::Y && modifiers.matches_logically(egui::Modifiers::COMMAND))
        || (*key == egui::Key::Z
            && modifiers.matches_logically(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT))
    {
        Some(true)
    } else if *key == egui::Key::Z && modifiers.matches_logically(egui::Modifiers::COMMAND) {
        Some(false)
    } else {
        None
    }
}

fn step_history(ctx: &egui::Context, doc: &mut Document, redo: bool) -> bool {
    let id = egui::Id::new(("editor", doc.id));
    let mut state = load(ctx, doc);
    let cursor = state
        .cursor
        .char_range()
        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)));
    let mut current = (cursor, doc.text.clone());
    let mut history = state.undoer();
    // The current in-flux snapshot can add one more state than the saved bound.
    for _ in 0..=MAX_UNDO_STATES {
        let next = if redo {
            history.redo(&current)
        } else {
            history.undo(&current)
        }
        .cloned();
        let Some(next) = next else {
            return false;
        };
        if next.1 != doc.text {
            let cursor = next.0.primary.index;
            state.set_undoer(history);
            state.cursor.set_char_range(Some(next.0));
            state.store(ctx, id);
            doc.text = next.1;
            doc.edit_version = doc.edit_version.saturating_add(1);
            doc.has_cjk |= crate::system_fonts::contains_cjk(&doc.text);
            doc.cursor = crate::model::cursor_location(&doc.text, cursor);
            doc.jump_to = None;
            doc.scroll_to = Some(cursor);
            return true;
        }
        current = next;
    }
    false
}

pub fn forget(ctx: &egui::Context, document: u64) {
    ctx.data_mut(|data| {
        data.remove::<egui::text_edit::TextEditState>(egui::Id::new(("editor", document)))
    });
}

pub fn commit(ctx: &egui::Context, doc: &mut Document, text: String, cursor_chars: usize) {
    commit_selection(
        ctx,
        doc,
        text,
        egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_chars)),
    );
}

/// Move a selection without adding a text checkpoint or losing the redo branch.
pub fn move_selection(
    ctx: &egui::Context,
    doc: &mut Document,
    selection: egui::text::CCursorRange,
) {
    let mut state = load(ctx, doc);
    state.set_undoer(replace_history_cursor(state.undoer(), selection, &doc.text));
    state.cursor.set_char_range(Some(selection));
    state.store(ctx, egui::Id::new(("editor", doc.id)));
    doc.cursor = crate::model::cursor_location(&doc.text, selection.primary.index);
    doc.scroll_to = Some(selection.primary.index);
}

/// One native history transaction retaining both endpoints and their affinity.
pub fn commit_selection(
    ctx: &egui::Context,
    doc: &mut Document,
    text: String,
    new_cursor: egui::text::CCursorRange,
) {
    let cursor_chars = new_cursor.primary.index;
    let id = egui::Id::new(("editor", doc.id));
    let mut state = load(ctx, doc);
    let old_cursor = state
        .cursor
        .char_range()
        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)));
    let before = (old_cursor, doc.text.clone());
    let after = (new_cursor, text.clone());
    // Clone history only for an explicit editor transaction, never per frame.
    let mut undoer = state.undoer();
    undoer.add_undo(&before);
    undoer.feed_state(ctx.input(|input| input.time), &after);
    undoer.add_undo(&after);
    state.set_undoer(undoer);
    state.cursor.set_char_range(Some(new_cursor));
    state.store(ctx, id);
    doc.has_cjk |= crate::system_fonts::contains_cjk(&text);
    doc.text = text;
    doc.edit_version = doc.edit_version.saturating_add(1);
    doc.cursor = crate::model::cursor_location(&doc.text, cursor_chars);
    doc.scroll_to = Some(cursor_chars);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_coalescing_preserves_sixteen_distinct_texts_and_complete_redo_branch() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file.rs".into(), "value 00".into(), "revision".into());
        for index in 1..MAX_UNDO_STATES {
            commit(&ctx, &mut doc, format!("value {index:02}"), 0);
        }
        for _ in 0..5 {
            assert!(step_history(&ctx, &mut doc, false));
        }
        assert_eq!(doc.text, "value 10");
        for index in 0..40 {
            let mut state = load(&ctx, &mut doc);
            let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(index % 2));
            let history = replace_history_cursor(state.undoer(), cursor, &doc.text);
            state.cursor.set_char_range(Some(cursor));
            state.set_undoer(history);
            state.store(&ctx, egui::Id::new(("editor", 1u64)));
        }
        for index in 11..MAX_UNDO_STATES {
            assert!(step_history(&ctx, &mut doc, true));
            assert_eq!(doc.text, format!("value {index:02}"));
        }
        assert!(!step_history(&ctx, &mut doc, true));
        for index in (0..MAX_UNDO_STATES - 1).rev() {
            assert!(step_history(&ctx, &mut doc, false));
            assert_eq!(doc.text, format!("value {index:02}"));
        }
        assert!(!step_history(&ctx, &mut doc, false));
        assert_eq!(doc.revision.as_deref(), Some("revision"));
    }
    #[test]
    fn cursor_coalescing_preserves_every_split_of_native_checkpoint_plus_in_flux_history() {
        let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(0));
        for undo_depth in 0..=MAX_UNDO_STATES {
            let mut history = EditorUndoer::with_settings(egui::util::undoer::Settings {
                max_undos: MAX_UNDO_STATES,
                ..Default::default()
            });
            for index in 0..MAX_UNDO_STATES {
                history.add_undo(&(cursor, format!("value {index}")));
            }
            let mut current = (cursor, format!("value {MAX_UNDO_STATES}"));
            for _ in 0..undo_depth {
                current = history.undo(&current).unwrap().clone();
            }
            let moved = egui::text::CCursorRange::one(egui::text::CCursor::new(1));
            history = replace_history_cursor(history, moved, &current.1);
            current.0 = moved;
            history.feed_state(10.0, &current);
            for index in MAX_UNDO_STATES - undo_depth + 1..=MAX_UNDO_STATES {
                current = history.redo(&current).unwrap().clone();
                assert_eq!(
                    current.1,
                    format!("value {index}"),
                    "Undo split {undo_depth}"
                );
            }
            assert!(history.redo(&current).is_none());
            // Full Redo legitimately leaves 17 native undo points. A later
            // selection move and idle feed must also preserve the oldest one.
            let after_redo = egui::text::CCursorRange::one(egui::text::CCursor::new(2));
            history = replace_history_cursor(history, after_redo, &current.1);
            current.0 = after_redo;
            history.feed_state(12.0, &current);
            for index in (0..MAX_UNDO_STATES).rev() {
                current = history.undo(&current).unwrap().clone();
                assert_eq!(
                    current.1,
                    format!("value {index}"),
                    "Undo split {undo_depth}"
                );
            }
            assert!(history.undo(&current).is_none());
            let after_undo = egui::text::CCursorRange::one(egui::text::CCursor::new(3));
            history = replace_history_cursor(history, after_undo, &current.1);
            current.0 = after_undo;
            for index in 1..=MAX_UNDO_STATES {
                current = history.redo(&current).unwrap().clone();
                assert_eq!(
                    current.1,
                    format!("value {index}"),
                    "Undo split {undo_depth}"
                );
            }
            assert!(history.redo(&current).is_none());
        }
    }
    #[test]
    fn idle_and_pointer_hover_do_not_snapshot_editor_history() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file.rs".into(), "before".into(), "revision".into());
        commit(&ctx, &mut doc, "after".into(), 0);
        assert!(step_history(&ctx, &mut doc, false));
        for frame in 0..50 {
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(f64::from(frame)),
                    events: if frame % 2 == 0 {
                        vec![]
                    } else {
                        vec![egui::Event::PointerMoved(egui::pos2(20.0, 20.0))]
                    },
                    ..Default::default()
                },
                |ctx| {
                    assert!(before_cursor_interaction(ctx, &mut doc, false).is_none());
                },
            );
        }
        assert!(step_history(&ctx, &mut doc, true));
        assert_eq!(doc.text, "after");
    }
    #[test]
    fn history_is_bounded_and_released_on_close() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file".into(), "start".into(), "r".into());
        for index in 0..40 {
            commit(&ctx, &mut doc, format!("edit {index}"), 0);
        }
        let state = load(&ctx, &mut doc);
        let mut undoer = state.undoer();
        let mut current = (state.cursor.char_range().unwrap(), doc.text.clone());
        let mut count = 0;
        while let Some(previous) = undoer.undo(&current).cloned() {
            current = previous;
            count += 1;
        }
        assert!(count <= MAX_UNDO_STATES);
        forget(&ctx, 1);
        assert!(egui::TextEdit::load_state(&ctx, egui::Id::new(("editor", 1u64))).is_none());
    }
    #[test]
    fn repeated_loading_does_not_reset_history() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file".into(), "before".into(), "r".into());
        commit(&ctx, &mut doc, "after".into(), 5);
        for _ in 0..100 {
            load(&ctx, &mut doc);
        }
        let state = load(&ctx, &mut doc);
        assert_eq!(
            state
                .undoer()
                .undo(&(state.cursor.char_range().unwrap(), doc.text.clone()))
                .unwrap()
                .1,
            "before"
        );
    }
    #[test]
    fn history_shortcuts_skip_cursor_only_checkpoints_without_touching_saved_state() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file.rs".into(), "before".into(), "revision".into());
        doc.saving = true;
        commit(&ctx, &mut doc, "after".into(), 0);
        let id = egui::Id::new(("editor", 1u64));
        let mut state = load(&ctx, &mut doc);
        let mut history = state.undoer();
        for index in 1..=4 {
            let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(index));
            history.add_undo(&(cursor, doc.text.clone()));
            state.cursor.set_char_range(Some(cursor));
        }
        state.set_undoer(history);
        state.store(&ctx, id);
        assert!(step_history(&ctx, &mut doc, false));
        assert_eq!(doc.text, "before");
        assert_eq!(doc.edit_version, 2);
        assert!(step_history(&ctx, &mut doc, true));
        assert_eq!(doc.text, "after");
        assert_eq!(doc.edit_version, 3);
        assert_eq!(doc.saved_text, "before");
        assert_eq!(doc.revision.as_deref(), Some("revision"));
        assert!(doc.saving);
    }

    #[test]
    fn only_cursor_history_does_not_mutate_document_or_version() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file.rs".into(), "text".into(), "revision".into());
        let mut state = load(&ctx, &mut doc);
        let mut history = state.undoer();
        for index in 0..4 {
            let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(index));
            history.add_undo(&(cursor, doc.text.clone()));
            state.cursor.set_char_range(Some(cursor));
        }
        state.set_undoer(history);
        state.store(&ctx, egui::Id::new(("editor", 1u64)));
        assert!(!step_history(&ctx, &mut doc, false));
        assert_eq!(doc.text, "text");
        assert_eq!(doc.edit_version, 0);
        assert!(!doc.dirty());
    }

    #[test]
    fn editor_does_not_capture_other_fields_history_keys() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "file.rs".into(), "before".into(), "revision".into());
        commit(&ctx, &mut doc, "after".into(), 0);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: Some(egui::Key::Z),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("other_field")));
                history_shortcut(ctx, &mut doc);
                assert!(ctx
                    .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)));
            },
        );
        assert_eq!(doc.text, "after");
        assert_eq!(doc.edit_version, 1);
    }
    #[test]
    fn mixed_and_oversized_history_batches_pass_through_unchanged() {
        let undo = egui::Event::Key {
            key: egui::Key::Z,
            physical_key: Some(egui::Key::Z),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        for events in [
            vec![egui::Event::Text("queued text".into()), undo.clone()],
            vec![
                egui::Event::PointerMoved(egui::pos2(10.0, 20.0)),
                undo.clone(),
            ],
            vec![undo; MAX_UNDO_STATES + 1],
        ] {
            let ctx = egui::Context::default();
            let mut doc = Document::new(1, "file.rs".into(), "before".into(), "revision".into());
            commit(&ctx, &mut doc, "after".into(), 0);
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
                    let before = ctx.input(|input| input.events.clone());
                    history_shortcut(ctx, &mut doc);
                    assert_eq!(ctx.input(|input| input.events.clone()), before);
                },
            );
            assert_eq!(doc.text, "after");
            assert_eq!(doc.edit_version, 1);
        }
    }
}
