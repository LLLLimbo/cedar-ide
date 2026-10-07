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
    // This clones history only for a deliberate completion transaction, never per frame.
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
}
