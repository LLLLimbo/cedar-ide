//! Selected-line indentation: bounded planning and narrow native-input ownership.
use crate::{editor_state, model::Document};
use eframe::egui::{self, text::CCursorRange};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINES: usize = 4096;

pub struct Plan {
    pub text: String,
    pub selection: CCursorRange,
}

/// Character-index endpoints stay attached to their original content. At an
/// inserted prefix they move after it; inside a deleted prefix they clamp to
/// its beginning. Copying the cursors preserves direction and row affinity.
pub fn plan(text: &str, selection: CCursorRange, outdent: bool) -> Option<Plan> {
    if text.len() > MAX_BYTES || selection.primary.index == selection.secondary.index {
        return None;
    }
    let low = selection.primary.index.min(selection.secondary.index);
    let high = selection.primary.index.max(selection.secondary.index);
    let chars = text.chars().count();
    if high > chars {
        return None;
    }
    let mut edits = Vec::new();
    let mut char_start = 0usize;
    let mut byte_start = 0usize;
    for line in text.split_inclusive('\n') {
        let char_end = char_start.checked_add(line.chars().count())?;
        if char_start < high && char_end > low {
            if edits.len() == MAX_LINES {
                return None;
            }
            let remove = if !outdent {
                0
            } else if line.starts_with('\t') {
                1
            } else {
                line.bytes().take(4).take_while(|b| *b == b' ').count()
            };
            edits.push((byte_start, char_start, remove));
        }
        char_start = char_end;
        byte_start = byte_start.checked_add(line.len())?;
        if char_start >= high {
            break;
        }
    }
    let removed = edits.iter().try_fold(0usize, |n, e| n.checked_add(e.2))?;
    let added = if outdent { 0 } else { edits.len() };
    let size = text.len().checked_sub(removed)?.checked_add(added)?;
    if size > MAX_BYTES || removed == 0 && added == 0 {
        return None;
    }
    let map = |index: usize| -> Option<usize> {
        let mut mapped = index;
        for &(_, start, remove) in &edits {
            if start > index {
                break;
            }
            if outdent {
                mapped = mapped.checked_sub((index - start).min(remove))?;
            } else {
                mapped = mapped.checked_add(1)?;
            }
        }
        Some(mapped)
    };
    let mut mapped = selection;
    mapped.primary.index = map(selection.primary.index)?;
    mapped.secondary.index = map(selection.secondary.index)?;
    let mut result = String::with_capacity(size);
    let mut previous = 0;
    for &(start, _, remove) in &edits {
        result.push_str(text.get(previous..start)?);
        if !outdent {
            result.push('\t');
        }
        previous = start.checked_add(remove)?;
    }
    result.push_str(text.get(previous..)?);
    Some(Plan {
        text: result,
        selection: mapped,
    })
}

fn tab(event: &egui::Event) -> Option<bool> {
    match event {
        egui::Event::Key {
            key: egui::Key::Tab,
            pressed: true,
            modifiers,
            ..
        } if !modifiers.alt && !modifiers.ctrl && !modifiers.command && !modifiers.mac_cmd => {
            Some(modifiers.shift)
        }
        _ => None,
    }
}

/// Refuse ambiguous event batches rather than replay native text input. Only
/// selected Tab presses belong to us; every other event retains its ordering.
/// Retained editor focus still owns a selected Tab when a form, jump, or focus
/// loss prevents execution: consume it without editing rather than passing it
/// to native selection replacement. A genuinely foreign-focused field owns all
/// of its own input, including Tab.
pub fn handle(ctx: &egui::Context, doc: &mut Document, eligible: bool) {
    let id = egui::Id::new(("editor", doc.id));
    let ime_id = id.with("block_indent_composition");
    let focused = ctx.memory(|m| m.has_focus(id));
    if !focused {
        ctx.data_mut(|d| d.remove::<bool>(ime_id));
        return;
    }
    let was_composing = ctx.data(|d| d.get_temp::<bool>(ime_id).unwrap_or(false));
    let mut composing = was_composing;
    ctx.input(|input| {
        for event in &input.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(text)) if text != "\n" && text != "\r" => {
                    composing = !text.is_empty()
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) if text != "\n" && text != "\r" => {
                    composing = false
                }
                egui::Event::Ime(egui::ImeEvent::Disabled) => composing = false,
                _ => {}
            }
        }
    });
    let viewport_focused = ctx.input(|input| input.focused);
    composing &= viewport_focused;
    ctx.data_mut(|d| d.insert_temp(ime_id, composing));
    if !ctx.input(|input| input.events.iter().any(|event| tab(event).is_some())) {
        return;
    }
    let Some(selection) = egui::TextEdit::load_state(ctx, id)
        .and_then(|state| state.cursor.char_range())
        .filter(|range| range.primary.index != range.secondary.index)
    else {
        return;
    };
    let eligible = eligible && viewport_focused;
    let action = ctx.input_mut(|input| {
        let mut action = None;
        let mut clean = eligible && !was_composing;
        for event in &input.events {
            if let Some(outdent) = tab(event) {
                if action.replace(outdent).is_some() {
                    clean = false;
                }
            } else if !matches!(
                event,
                egui::Event::Key { pressed: false, .. }
                    | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
            ) {
                clean = false;
            }
        }
        input.events.retain(|event| tab(event).is_none());
        action.filter(|_| clean)
    });
    if let Some(outdent) = action {
        if let Some(plan) = plan(&doc.text, selection, outdent) {
            editor_state::commit_selection(ctx, doc, plan.text, plan.selection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::text::CCursor;

    fn selection(primary: usize, secondary: usize) -> CCursorRange {
        CCursorRange {
            primary: CCursor {
                index: primary,
                prefer_next_row: true,
            },
            secondary: CCursor {
                index: secondary,
                prefer_next_row: false,
            },
        }
    }

    fn assert_selection(actual: CCursorRange, expected: CCursorRange) {
        assert_eq!(actual, expected);
        assert_eq!(
            actual.primary.prefer_next_row,
            expected.primary.prefer_next_row
        );
        assert_eq!(
            actual.secondary.prefer_next_row,
            expected.secondary.prefer_next_row
        );
    }

    #[test]
    fn unicode_crlf_reverse_and_endpoint_line_exclusion() {
        let source = "é😀\r\n  β\r\nlast\n";
        let range = selection(0, 9);
        let result = plan(source, range, false).unwrap();
        assert_eq!(result.text, "\té😀\r\n\t  β\r\nlast\n");
        assert_selection(result.selection, selection(1, 11));
        let restored = plan(&result.text, result.selection, true).unwrap();
        assert_eq!(restored.text, source);
        assert_selection(restored.selection, range);
    }

    #[test]
    fn blank_lines_final_newline_partial_selection_and_mixed_prefixes() {
        let result = plan("one\n\nlast\n", selection(9, 1), false).unwrap();
        assert_eq!(result.text, "\tone\n\t\n\tlast\n");
        assert_selection(result.selection, selection(12, 2));
        let source = " \tx\n   y\n     z\n\t q";
        let result = plan(source, selection(source.chars().count(), 1), true).unwrap();
        assert_eq!(result.text, "\tx\ny\n z\n q");
        assert_eq!(result.selection.secondary.index, 0);
        assert_eq!(result.selection.primary.index, result.text.chars().count());
    }

    #[test]
    fn input_result_line_bounds_and_invalid_ranges_fail_closed() {
        assert!(plan("abc", selection(1, 1), false).is_none());
        assert!(plan("abc", selection(4, 0), false).is_none());
        assert!(plan("abc", selection(usize::MAX, 0), false).is_none());
        assert!(plan("abc", selection(2, 0), true).is_none());
        let exact = "x".repeat(MAX_BYTES);
        assert!(plan(&exact, selection(1, 0), false).is_none());
        let over = format!(" {exact}");
        assert!(plan(&over, selection(1, 0), true).is_none());
        let fits = "x".repeat(MAX_BYTES - 1);
        assert_eq!(
            plan(&fits, selection(1, 0), false).unwrap().text.len(),
            MAX_BYTES
        );
        let lines = "x\n".repeat(MAX_LINES);
        assert!(plan(&lines, selection(lines.len(), 0), false).is_some());
        let too_many = format!("{lines}x");
        assert!(plan(&too_many, selection(too_many.len(), 0), false).is_none());
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn frame(app: &mut crate::CedarApp, events: Vec<egui::Event>) {
        let ctx = app.editor_ctx.clone();
        let time = ctx.input(|i| i.time) + 1.0;
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1320.0, 880.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(app, ctx, &mut native),
        );
    }

    fn app(text: &str, range: CCursorRange) -> crate::CedarApp {
        let mut app = crate::CedarApp::empty();
        app.open_form = false;
        app.documents.push(Document::new(
            1,
            "main.rs".into(),
            text.into(),
            "rev".into(),
        ));
        app.active_document = Some(1);
        frame(&mut app, vec![]);
        select(&mut app, range);
        frame(&mut app, vec![]);
        app
    }

    fn select(app: &mut crate::CedarApp, range: CCursorRange) {
        let ctx = &app.editor_ctx;
        let id = egui::Id::new(("editor", 1u64));
        let mut state = editor_state::load(ctx, &mut app.documents[0]);
        state.cursor.set_char_range(Some(range));
        state.store(ctx, id);
        ctx.memory_mut(|m| m.request_focus(id));
    }

    fn range(app: &crate::CedarApp) -> CCursorRange {
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
    }

    #[test]
    fn actual_frames_indent_outdent_and_one_effective_undo_redo() {
        let source = "é😀\r\n  β\r\nlast\n";
        let before = selection(0, 9);
        let mut app = app(source, before);
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(app.documents[0].text, "\té😀\r\n\t  β\r\nlast\n");
        assert_eq!(app.documents[0].edit_version, 1);
        assert_selection(range(&app), selection(1, 11));
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_selection(range(&app), before);
        assert_eq!(app.documents[0].edit_version, 2);
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].edit_version, 3);
        assert_selection(range(&app), selection(1, 11));
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)]);
        assert_eq!(app.documents[0].text, source);
        assert_selection(range(&app), before);
        assert_eq!(app.documents[0].edit_version, 4);
        assert_eq!(app.documents[0].saved_text, source);
        assert_eq!(app.documents[0].revision.as_deref(), Some("rev"));
    }

    #[test]
    fn actual_frames_mixed_input_consumes_tab_preserving_native_text_order() {
        for events in [
            vec![
                key(egui::Key::Tab, egui::Modifiers::NONE),
                egui::Event::Text("A".into()),
                egui::Event::Paste("B".into()),
            ],
            vec![
                egui::Event::Text("A".into()),
                key(egui::Key::Tab, egui::Modifiers::NONE),
                egui::Event::Paste("B".into()),
            ],
            vec![
                egui::Event::Text("A".into()),
                egui::Event::Paste("B".into()),
                key(egui::Key::Tab, egui::Modifiers::NONE),
            ],
        ] {
            let mut app = app("abc\ndef", selection(3, 0));
            frame(&mut app, events);
            assert_eq!(app.documents[0].text, "AB\ndef");
            assert_eq!(app.documents[0].edit_version, 1);
        }
    }

    #[test]
    fn actual_frames_housekeeping_ime_allows_tab_active_preedit_refuses() {
        for housekeeping in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
            let mut app = app("abc", selection(3, 0));
            frame(
                &mut app,
                vec![
                    egui::Event::Ime(housekeeping),
                    key(egui::Key::Tab, egui::Modifiers::NONE),
                ],
            );
            assert_eq!(app.documents[0].text, "\tabc");
            assert_eq!(app.documents[0].edit_version, 1);
        }
        let mut app = app("abc", selection(3, 0));
        frame(
            &mut app,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
            ],
        );
        let text = app.documents[0].text.clone();
        let version = app.documents[0].edit_version;
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(app.documents[0].text, text);
        assert_eq!(app.documents[0].edit_version, version);
        frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
        select(&mut app, selection(1, 0));
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(app.documents[0].text, format!("\t{text}"));
    }

    #[test]
    fn actual_frames_forms_modals_other_focus_and_empty_selection() {
        for blocker in 0..5 {
            let mut app = app("abc\ndef", selection(3, 0));
            match blocker {
                0 => app.open_form = true,
                1 => app.new_file = true,
                2 => app.confirm = Some(crate::Confirm::CloseWindow),
                3 => app.documents[0].jump_to = Some(4),
                _ => {
                    app.find_open = true;
                    app.find_focus = true;
                }
            }
            frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
            assert_eq!(app.documents[0].text, "abc\ndef", "blocker {blocker}");
            assert_eq!(app.documents[0].edit_version, 0);
            if blocker != 3 {
                assert_selection(range(&app), selection(3, 0));
            }
        }
        let mut app = app("abc", selection(1, 1));
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(app.documents[0].text, "a\tbc");
        assert_eq!(app.documents[0].edit_version, 1);
    }

    #[test]
    fn actual_frames_inactive_viewport_does_not_feed_native_tab() {
        let mut app = app("abc", selection(3, 0));
        let ctx = app.editor_ctx.clone();
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                focused: false,
                events: vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut native),
        );
        assert_eq!(app.documents[0].text, "abc");
        assert_eq!(app.documents[0].edit_version, 0);
    }

    #[test]
    fn ignored_newline_preedit_and_blocked_cancellation_do_not_leave_stale_composition() {
        for newline in ["\n", "\r"] {
            let mut app = app("abc", selection(3, 0));
            frame(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit(newline.into()))],
            );
            frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
            assert_eq!(app.documents[0].text, "\tabc");
        }
        let mut app = app("abc", selection(3, 0));
        frame(
            &mut app,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
            ],
        );
        let ctx = app.editor_ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
                ..Default::default()
            },
            |ctx| handle(ctx, &mut app.documents[0], false),
        );
        select(&mut app, selection(1, 0));
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(app.documents[0].text, "\t仮");
    }

    #[test]
    fn actual_frames_refused_caps_multiple_tabs_and_repeated_key() {
        let mut app = app("abc", selection(3, 0));
        frame(
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE); 2],
        );
        assert_eq!(app.documents[0].text, "abc");
        assert_eq!(app.documents[0].edit_version, 0);
        let mut repeat = key(egui::Key::Tab, egui::Modifiers::NONE);
        if let egui::Event::Key { repeat, .. } = &mut repeat {
            *repeat = true;
        }
        frame(&mut app, vec![repeat]);
        assert_eq!(app.documents[0].text, "\tabc");
        assert_eq!(app.documents[0].edit_version, 1);
        let ctx = app.editor_ctx.clone();
        let over = "x\n".repeat(MAX_LINES + 1);
        app.documents[0].text = over.clone();
        let mut state = editor_state::load(&ctx, &mut app.documents[0]);
        state.cursor.set_char_range(Some(selection(over.len(), 0)));
        state.store(&ctx, egui::Id::new(("editor", 1u64)));
        // Exercise the production handler without laying out thousands of rows.
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new(("editor", 1u64))));
                handle(ctx, &mut app.documents[0], true);
                assert!(ctx.input(|i| i.events.is_empty()));
            },
        );
        assert_eq!(app.documents[0].text, over);
        assert_eq!(app.documents[0].edit_version, 1);
    }

    #[test]
    fn refused_batches_leave_unrelated_events_exactly_and_noop_keeps_redo() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(1, "main.rs".into(), "abc".into(), "rev".into());
        let id = egui::Id::new(("editor", 1u64));
        let mut state = editor_state::load(&ctx, &mut doc);
        state.cursor.set_char_range(Some(selection(3, 0)));
        state.store(&ctx, id);
        let other = vec![
            egui::Event::Copy,
            key(egui::Key::ArrowLeft, egui::Modifiers::NONE),
            egui::Event::Paste("z".into()),
        ];
        let mut events = other.clone();
        events.insert(1, key(egui::Key::Tab, egui::Modifiers::SHIFT));
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.request_focus(id));
                handle(ctx, &mut doc, true);
                assert_eq!(ctx.input(|i| i.events.clone()), other);
            },
        );
        assert_eq!(doc.text, "abc");
        assert_eq!(doc.edit_version, 0);
        let unrelated = vec![
            egui::Event::Text("queued".into()),
            egui::Event::Paste("draft".into()),
        ];
        let mut blocked_events = unrelated.clone();
        blocked_events.insert(1, key(egui::Key::Tab, egui::Modifiers::NONE));
        let _ = ctx.run(
            egui::RawInput {
                events: blocked_events,
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.request_focus(id));
                handle(ctx, &mut doc, false);
                assert_eq!(ctx.input(|i| i.events.clone()), unrelated);
            },
        );
        assert_eq!(doc.text, "abc");
        assert_eq!(doc.edit_version, 0);
        let tab_event = key(egui::Key::Tab, egui::Modifiers::NONE);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![tab_event.clone()],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("other_field")));
                let before = ctx.input(|i| i.events.clone());
                handle(ctx, &mut doc, true);
                assert_eq!(ctx.input(|i| i.events.clone()), before);
            },
        );
        assert_eq!(doc.text, "abc");

        let mut app = app("abc", selection(3, 0));
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        let version = app.documents[0].edit_version;
        frame(&mut app, vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)]);
        assert_eq!(app.documents[0].edit_version, version);
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, "\tabc");
    }
}
