//! Bounded native editor history, initialized once for each document.
use crate::model::Document;
use eframe::egui;
pub const MAX_UNDO_STATES: usize = 16;

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
    let id = egui::Id::new(("editor", doc.id));
    let mut state = load(ctx, doc);
    let old_cursor = state
        .cursor
        .char_range()
        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)));
    let before = (old_cursor, doc.text.clone());
    let new_cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_chars));
    let after = (new_cursor, text.clone());
    // This clones history only for a deliberate language-edit transaction, never per frame.
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
