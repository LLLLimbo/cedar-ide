//! Bounded Enter continuation of the ASCII indentation before the selection.
use crate::{editor_state, model::Document};
use eframe::egui::{self, text::CCursorRange};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_PREFIX_BYTES: usize = 4096;

pub struct Plan {
    pub text: String,
    pub selection: CCursorRange,
}

/// A one-pass grant captured before the raw-input hook consumes tree-owned keys.
/// The app takes it at the start of update even when no editor will be drawn.
pub struct FrameInput {
    pass: u64,
    document: u64,
    version: u64,
}

#[derive(Clone, Copy)]
struct Composition {
    document: u64,
    active: bool,
}

fn composition_id() -> egui::Id {
    egui::Id::new("enter_indent_composition")
}

pub fn capture(
    ctx: &egui::Context,
    input: &egui::RawInput,
    doc: Option<&Document>,
) -> Option<FrameInput> {
    let focused = input.focused
        && doc.is_some_and(|doc| {
            ctx.memory(|memory| memory.has_focus(egui::Id::new(("editor", doc.id))))
        });
    if !focused {
        ctx.data_mut(|data| data.remove::<Composition>(composition_id()));
    }
    let doc = doc?;
    (focused && clean_enter(&input.events)).then(|| FrameInput {
        pass: ctx.cumulative_pass_nr(),
        document: doc.id,
        version: doc.edit_version,
    })
}

/// Endpoints are character offsets. Only indentation that survives before the
/// lower endpoint is copied; the remaining suffix is preserved byte for byte.
pub fn plan(text: &str, selection: CCursorRange) -> Option<Plan> {
    if text.len() > MAX_BYTES {
        return None;
    }
    let mut low = selection.primary.index.min(selection.secondary.index);
    let high = selection.primary.index.max(selection.secondary.index);
    let mut low_byte = None;
    let mut high_byte = None;
    for (index, byte) in text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain([text.len()])
        .enumerate()
    {
        if index == low {
            low_byte = Some(byte);
        }
        if index == high {
            high_byte = Some(byte);
            break;
        }
    }
    let mut low_byte = low_byte?;
    let mut high_byte = high_byte?;
    let inside_crlf = |byte: usize| {
        byte > 0
            && text.as_bytes().get(byte - 1) == Some(&b'\r')
            && text.as_bytes().get(byte) == Some(&b'\n')
    };
    if inside_crlf(low_byte) || inside_crlf(high_byte) {
        if low != high {
            return None;
        }
        // Native End can put a collapsed caret between CR and LF. For this
        // insertion only, use the logical end before CR; the original cursor
        // (including both affinities) remains the transaction's Undo state.
        low = low.checked_sub(1)?;
        low_byte = low_byte.checked_sub(1)?;
        high_byte = low_byte;
    }
    let line_start = text[..low_byte].rfind('\n').map_or(0, |byte| byte + 1);
    let prefix_len = text.as_bytes()[line_start..low_byte]
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    if prefix_len > MAX_PREFIX_BYTES {
        return None;
    }
    // Prefer the insertion line's terminator even if the selection removes it.
    // An unterminated final line inherits its nearest preceding LF or CRLF.
    let terminator = text[low_byte..]
        .find('\n')
        .map(|byte| low_byte + byte)
        .or_else(|| line_start.checked_sub(1));
    let newline = if terminator.is_some_and(|byte| byte > 0 && text.as_bytes()[byte - 1] == b'\r') {
        "\r\n"
    } else {
        "\n"
    };
    let inserted = newline.len().checked_add(prefix_len)?;
    let result_len = text
        .len()
        .checked_sub(high_byte - low_byte)?
        .checked_add(inserted)?;
    if result_len > MAX_BYTES {
        return None;
    }
    let cursor = low.checked_add(inserted)?; // Inserted bytes are all ASCII.
    let mut result = String::with_capacity(result_len);
    result.push_str(&text[..low_byte]);
    result.push_str(newline);
    result.push_str(&text[line_start..line_start + prefix_len]);
    result.push_str(&text[high_byte..]);
    Some(Plan {
        text: result,
        // A new insertion caret has no inherited wrapped-row affinity.
        selection: CCursorRange::one(egui::text::CCursor::new(cursor)),
    })
}

fn plain_enter(event: &egui::Event) -> bool {
    matches!(event, egui::Event::Key {
        key: egui::Key::Enter, pressed: true, modifiers, ..
    } if *modifiers == egui::Modifiers::NONE)
}

fn clean_enter(events: &[egui::Event]) -> bool {
    let mut found = false;
    for event in events {
        if plain_enter(event) {
            if found {
                return false;
            }
            found = true;
        } else if !matches!(
            event,
            egui::Event::Key { pressed: false, .. }
                | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
        ) {
            return false;
        }
    }
    found
}

/// Native text, paste, IME and mixed batches remain untouched. Read the original
/// egui frame as well as the remaining events so another interceptor cannot
/// turn an ambiguous batch into an isolated Enter by consuming part of it.
pub fn handle(
    ctx: &egui::Context,
    doc: &mut Document,
    eligible: bool,
    original: Option<FrameInput>,
) {
    let id = egui::Id::new(("editor", doc.id));
    let ime_id = composition_id();
    if !ctx.memory(|memory| memory.has_focus(id)) {
        ctx.data_mut(|data| data.remove::<Composition>(ime_id));
        return;
    }
    let was_composing = ctx.data(|data| {
        data.get_temp::<Composition>(ime_id)
            .is_some_and(|composition| composition.document == doc.id && composition.active)
    });
    let mut composing = was_composing;
    ctx.input(|input| {
        for event in &input.raw.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(text)) if text != "\n" && text != "\r" => {
                    composing = !text.is_empty();
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) if text != "\n" && text != "\r" => {
                    composing = false;
                }
                egui::Event::Ime(egui::ImeEvent::Disabled) => composing = false,
                _ => {}
            }
        }
        composing &= input.focused;
    });
    ctx.data_mut(|data| {
        data.insert_temp(
            ime_id,
            Composition {
                document: doc.id,
                active: composing,
            },
        )
    });
    if !original.is_some_and(|original| {
        original.pass == ctx.cumulative_pass_nr()
            && original.document == doc.id
            && original.version == doc.edit_version
    }) || !eligible
        || !ctx.input(|input| input.focused)
        || was_composing
        || !ctx.input(|input| clean_enter(&input.raw.events) && clean_enter(&input.events))
    {
        return;
    }
    // An owned Enter that cannot be planned must not fall through to native
    // selection deletion. Ineligible and foreign-focused input stays native.
    ctx.input_mut(|input| input.events.retain(|event| !plain_enter(event)));
    if doc.edit_version == u64::MAX {
        return;
    }
    let Some(selection) =
        egui::TextEdit::load_state(ctx, id).and_then(|state| state.cursor.char_range())
    else {
        return;
    };
    if let Some(plan) = plan(&doc.text, selection) {
        if plan.text == doc.text {
            editor_state::move_selection(ctx, doc, plan.selection);
        } else {
            // egui cursor equality ignores affinity, so add_undo alone can
            // retain a stale before-cursor at identical character endpoints.
            editor_state::move_selection(ctx, doc, selection);
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

    fn caret(index: usize) -> CCursorRange {
        CCursorRange::one(CCursor::new(index))
    }

    #[test]
    fn surviving_ascii_prefix_and_unmodified_suffix() {
        for (source, index, expected, cursor) in [
            ("", 0, "\n", 1),
            ("    x", 2, "  \n    x", 5),
            (" \t", 2, " \t\n \t", 5),
            ("  value tail", 7, "  value\n   tail", 10),
            ("\t  {", 4, "\t  {\n\t  ", 8),
            ("  // text", 9, "  // text\n  ", 12),
            ("\u{a0} x", 3, "\u{a0} x\n", 4),
            ("  x\ry", 5, "  x\ry\n  ", 8),
            ("\n", 1, "\n\n", 2),
        ] {
            let planned = plan(source, caret(index)).unwrap();
            assert_eq!(planned.text, expected, "source {source:?}");
            assert_selection(planned.selection, caret(cursor));
        }
        let source = " \té😀\r\n \tβtail\r\nlast";
        for range in [selection(8, 3), selection(3, 8)] {
            let planned = plan(source, range).unwrap();
            assert_eq!(planned.text, " \té\r\n \tβtail\r\nlast");
            assert_selection(planned.selection, caret(7));
        }
        let planned = plan("    left\n\t right", selection(13, 2)).unwrap();
        assert_eq!(planned.text, "  \n  ght");
        assert_selection(planned.selection, caret(5));
    }

    #[test]
    fn insertion_line_then_nearest_preceding_terminator() {
        for (source, index, expected) in [
            ("  a\r\nb\n", 3, "  a\r\n  \r\nb\n"),
            ("  a\nb\r\n", 3, "  a\n  \nb\r\n"),
            ("a\r\nb\n  c", 8, "a\r\nb\n  c\n  "),
            ("a\nb\r\n  c", 8, "a\nb\r\n  c\r\n  "),
            ("a\r\n", 3, "a\r\n\r\n"),
            ("a\n", 2, "a\n\n"),
        ] {
            let planned = plan(source, caret(index)).unwrap();
            assert_eq!(planned.text, expected, "source {source:?}");
        }
        // The selected CRLF still determines the insertion line's newline.
        assert_eq!(
            plan("  a\r\nb\n", selection(6, 3)).unwrap().text,
            "  a\r\n  \n"
        );
    }

    #[test]
    fn exact_limits_invalid_offsets_and_crlf_interiors() {
        for range in [
            selection(usize::MAX, 0),
            selection(0, 5),
            selection(2, 0),
            selection(0, 2),
        ] {
            assert!(plan("é\r\n😀", range).is_none(), "range {range:?}");
        }
        assert!(plan("é\r\n😀", caret(1)).is_some());
        assert!(plan("é\r\n😀", caret(2)).is_some());
        assert!(plan("é\r\n😀", caret(3)).is_some());
        let prefix = " ".repeat(MAX_PREFIX_BYTES);
        assert!(plan(&prefix, caret(prefix.len())).is_some());
        let over_prefix = format!("{prefix} ");
        assert!(plan(&over_prefix, caret(over_prefix.len())).is_none());
        assert!(plan(&over_prefix, caret(2)).is_some());
        let fits = "x".repeat(MAX_BYTES - 1);
        assert_eq!(
            plan(&fits, caret(fits.len())).unwrap().text.len(),
            MAX_BYTES
        );
        let exact = format!("{fits}x");
        assert!(plan(&exact, caret(exact.len())).is_none());
        assert_eq!(plan(&exact, selection(1, 0)).unwrap().text.len(), MAX_BYTES);
        assert!(plan(&format!("{exact}x"), selection(MAX_BYTES + 1, 0)).is_none());
        let exact_crlf = format!("\r\n{}", "x".repeat(MAX_BYTES - 3));
        assert!(plan(&exact_crlf, caret(exact_crlf.len())).is_none());
    }

    #[test]
    fn only_collapsed_crlf_interior_planning_uses_logical_line_end() {
        for (source, index, expected, after) in [
            ("\r\n", 1, "\r\n\r\n", 2),
            ("é\r\n😀", 2, "é\r\n\r\n😀", 3),
            (" \té😀\r\nz", 5, " \té😀\r\n \t\r\nz", 8),
            ("\r\r\n", 2, "\r\r\n\r\n", 3),
        ] {
            for primary_affinity in [false, true] {
                for secondary_affinity in [false, true] {
                    let mut selected = caret(index);
                    selected.primary.prefer_next_row = primary_affinity;
                    selected.secondary.prefer_next_row = secondary_affinity;
                    let planned = plan(source, selected).unwrap();
                    assert_eq!(planned.text, expected, "source {source:?}");
                    assert_selection(planned.selection, caret(after));
                }
            }
        }
        for selected in [
            selection(1, 2),
            selection(2, 1),
            selection(2, 3),
            selection(3, 2),
        ] {
            assert!(plan("é\r\n😀", selected).is_none());
        }
        for (source, index, expected, after) in [
            ("é\r\n😀", 1, "é\r\n\r\n😀", 3),
            ("é\r\n😀", 3, "é\r\n\r\n😀", 5),
            ("  é\r😀", 3, "  é\n  \r😀", 6),
            ("  é\r😀", 4, "  é\r\n  😀", 7),
        ] {
            let planned = plan(source, caret(index)).unwrap();
            assert_eq!(planned.text, expected, "source {source:?}");
            assert_selection(planned.selection, caret(after));
        }
        let fits = format!("{}\r\n", "x".repeat(MAX_BYTES - 4));
        assert_eq!(
            plan(&fits, caret(fits.len() - 1)).unwrap().text.len(),
            MAX_BYTES
        );
        let exact = format!("{}\r\n", "x".repeat(MAX_BYTES - 2));
        assert!(plan(&exact, caret(exact.len() - 1)).is_none());
        let over_prefix = format!("{}\r\n", " ".repeat(MAX_PREFIX_BYTES + 1));
        assert!(plan(&over_prefix, caret(over_prefix.len() - 1)).is_none());
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

    fn enter() -> egui::Event {
        key(egui::Key::Enter, egui::Modifiers::NONE)
    }

    fn raw(ctx: &egui::Context, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 1.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1320.0, 880.0),
            )),
            events,
            ..Default::default()
        }
    }

    fn frame(app: &mut crate::CedarApp, events: Vec<egui::Event>) {
        let ctx = app.editor_ctx.clone();
        let mut input = raw(&ctx, events);
        eframe::App::raw_input_hook(app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    }

    fn select(app: &mut crate::CedarApp, range: CCursorRange) {
        let ctx = &app.editor_ctx;
        let id = egui::Id::new(("editor", 1u64));
        let mut state = editor_state::load(ctx, &mut app.documents[0]);
        state.cursor.set_char_range(Some(range));
        state.store(ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
    }

    fn app(text: &str, range: CCursorRange) -> crate::CedarApp {
        let mut app = crate::CedarApp::empty();
        app.open_form = false;
        app.documents.push(Document::new(
            1,
            "main.rs".into(),
            text.into(),
            "revision".into(),
        ));
        app.active_document = Some(1);
        frame(&mut app, vec![]);
        select(&mut app, range);
        app
    }

    fn range(app: &crate::CedarApp) -> CCursorRange {
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
    }

    #[test]
    fn actual_frames_reverse_unicode_crlf_atomic_undo_redo() {
        let source = " \té😀\r\n \tβtail\r\nlast";
        let before = selection(8, 3);
        let mut app = app(source, before);
        app.documents[0].saving = true;
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, " \té\r\n \tβtail\r\nlast");
        assert_eq!(app.documents[0].edit_version, 1);
        assert_selection(range(&app), caret(7));
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, 2);
        assert_selection(range(&app), before);
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, " \té\r\n \tβtail\r\nlast");
        assert_eq!(app.documents[0].edit_version, 3);
        assert_selection(range(&app), caret(7));
        assert_eq!(app.documents[0].saved_text, source);
        assert_eq!(app.documents[0].revision.as_deref(), Some("revision"));
        assert!(app.documents[0].saving);
    }

    #[test]
    fn actual_end_then_enter_continues_crlf_line() {
        let source = "    café λ\r\n    tail\r\n";
        let mut app = app(source, caret(source.chars().count()));
        frame(
            &mut app,
            vec![key(
                egui::Key::Home,
                egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
            )],
        );
        assert_eq!(range(&app).primary.index, 0);
        frame(&mut app, vec![key(egui::Key::End, egui::Modifiers::NONE)]);
        // egui's native End includes the CR in its visual line, placing the
        // caret between CR and LF. No synthetic selection is installed here.
        assert_eq!(range(&app).primary.index, 11);
        assert_eq!(range(&app).secondary.index, 11);
        let before = range(&app);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, 0);
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, "    café λ\r\n    \r\n    tail\r\n");
        assert_eq!(app.documents[0].edit_version, 1);
        assert_selection(range(&app), caret(16));
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, 2);
        assert_selection(range(&app), before);
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, "    café λ\r\n    \r\n    tail\r\n");
        assert_eq!(app.documents[0].edit_version, 3);
        assert_selection(range(&app), caret(16));
    }

    #[test]
    fn actual_shift_end_crlf_selection_still_refuses_enter() {
        let source = "    café λ\r\n    tail\r\n";
        let mut app = app(source, caret(0));
        frame(&mut app, vec![key(egui::Key::End, egui::Modifiers::SHIFT)]);
        let before = range(&app);
        assert_eq!(before.primary.index, 11);
        assert_eq!(before.secondary.index, 0);
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, 0);
        assert_selection(range(&app), before);
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, 0);
    }

    #[test]
    fn collapsed_crlf_interior_restores_distinct_endpoint_affinities() {
        let source = " \té😀\r\nz";
        for primary_affinity in [false, true] {
            for secondary_affinity in [false, true] {
                let mut before = caret(5);
                before.primary.prefer_next_row = primary_affinity;
                before.secondary.prefer_next_row = secondary_affinity;
                let mut app = app(source, before);
                frame(&mut app, vec![enter()]);
                assert_eq!(app.documents[0].text, " \té😀\r\n \t\r\nz");
                assert_eq!(app.documents[0].edit_version, 1);
                assert_selection(range(&app), caret(8));
                frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
                assert_eq!(app.documents[0].text, source);
                assert_eq!(app.documents[0].edit_version, 2);
                assert_selection(range(&app), before);
                frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
                assert_eq!(app.documents[0].text, " \té😀\r\n \t\r\nz");
                assert_eq!(app.documents[0].edit_version, 3);
                assert_selection(range(&app), caret(8));
            }
        }
    }

    #[test]
    fn identical_replacement_collapses_without_text_history_or_redo_loss() {
        let source = "  x\n  y";
        let mut app = app(source, caret(7));
        frame(&mut app, vec![enter()]);
        let edited = app.documents[0].text.clone();
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        select(&mut app, selection(3, 6));
        let version = app.documents[0].edit_version;
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, version);
        assert_selection(range(&app), caret(6));
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, edited);
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_selection(range(&app), caret(6));
        let version = app.documents[0].edit_version;
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_eq!(app.documents[0].edit_version, version);
    }

    #[test]
    fn same_index_affinity_change_is_the_exact_enter_undo_checkpoint() {
        let source = "  abc";
        let before = selection(2, 5);
        let mut stale = before;
        stale.primary.prefer_next_row = false;
        stale.secondary.prefer_next_row = true;
        let mut app = app(source, stale);
        let ctx = app.editor_ctx.clone();
        let mut state = editor_state::load(&ctx, &mut app.documents[0]);
        let mut history = state.undoer();
        history.add_undo(&(stale, source.into()));
        state.set_undoer(history);
        state.cursor.set_char_range(Some(before));
        state.store(&ctx, egui::Id::new(("editor", 1u64)));
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, "  \n  ");
        assert_eq!(app.documents[0].edit_version, 1);
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_selection(range(&app), before);
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, "  \n  ");
        assert_selection(range(&app), caret(5));
    }

    #[test]
    fn identical_replacement_updates_same_index_affinity_without_losing_redo() {
        let source = "  x\n  y";
        let mut old = caret(6);
        old.primary.prefer_next_row = true;
        old.secondary.prefer_next_row = true;
        let mut app = app(source, old);
        let ctx = app.editor_ctx.clone();
        editor_state::commit_selection(&ctx, &mut app.documents[0], format!("{source}!"), caret(8));
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_selection(range(&app), old);
        select(&mut app, selection(3, 6));
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].edit_version, 2);
        assert_selection(range(&app), caret(6));
        frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, format!("{source}!"));
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, source);
        assert_selection(range(&app), caret(6));
        assert_eq!(app.documents[0].edit_version, 4);
    }

    #[test]
    fn selected_tab_enter_and_native_mixed_input_never_gain_continuation() {
        for events in [
            vec![key(egui::Key::Tab, egui::Modifiers::NONE), enter()],
            vec![enter(), key(egui::Key::Tab, egui::Modifiers::NONE)],
            vec![
                enter(),
                egui::Event::Text("A".into()),
                egui::Event::Paste("B".into()),
            ],
            vec![
                egui::Event::Text("A".into()),
                enter(),
                egui::Event::Paste("B".into()),
            ],
            vec![
                egui::Event::Text("A".into()),
                egui::Event::Paste("B".into()),
                enter(),
            ],
            vec![enter(), enter()],
        ] {
            let mut actual = app("  abc\ndef", selection(5, 2));
            let mut native = app("  abc\ndef", selection(5, 2));
            frame(&mut actual, events.clone());
            let ctx = native.editor_ctx.clone();
            let input = raw(&ctx, events);
            let mut frame = eframe::Frame::_new_kittest();
            let _ = ctx.run(input, |ctx| {
                eframe::App::update(&mut native, ctx, &mut frame)
            });
            assert_eq!(actual.documents[0].text, native.documents[0].text);
            assert_selection(range(&actual), range(&native));
            assert_eq!(
                actual.documents[0].edit_version,
                native.documents[0].edit_version
            );
        }
    }

    #[test]
    fn strict_modifiers_repeated_key_and_housekeeping() {
        for bit in 0..5 {
            let mut modifiers = egui::Modifiers::NONE;
            match bit {
                0 => modifiers.alt = true,
                1 => modifiers.ctrl = true,
                2 => modifiers.shift = true,
                3 => modifiers.mac_cmd = true,
                _ => modifiers.command = true,
            }
            assert!(!clean_enter(&[key(egui::Key::Enter, modifiers)]));
        }
        for housekeeping in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
            let mut app = app(" \tx", caret(3));
            let mut repeat = enter();
            if let egui::Event::Key { repeat, .. } = &mut repeat {
                *repeat = true;
            }
            let mut release = key(egui::Key::A, egui::Modifiers::SHIFT);
            if let egui::Event::Key { pressed, .. } = &mut release {
                *pressed = false;
            }
            frame(
                &mut app,
                vec![egui::Event::Ime(housekeeping), release, repeat],
            );
            assert_eq!(app.documents[0].text, " \tx\n \t");
            assert_eq!(app.documents[0].edit_version, 1);
        }
    }

    fn handler_frame(
        app: &mut crate::CedarApp,
        events: Vec<egui::Event>,
        eligible: bool,
    ) -> Vec<egui::Event> {
        let ctx = app.editor_ctx.clone();
        let input = raw(&ctx, events);
        let mut original = capture(&ctx, &input, app.active());
        let mut remaining = Vec::new();
        let _ = ctx.run(input, |ctx| {
            handle(ctx, &mut app.documents[0], eligible, original.take());
            remaining = ctx.input(|input| input.events.clone());
        });
        remaining
    }

    #[test]
    fn handler_refuses_owned_invalid_ranges_limits_and_saturated_version() {
        for (text, selected, saturated) in [
            ("é\r\n😀".into(), selection(2, 0), false),
            ("é\r\n😀".into(), selection(0, 2), false),
            ("abc".into(), selection(usize::MAX, 0), false),
            ("x".repeat(MAX_BYTES), caret(MAX_BYTES), false),
            ("x".repeat(MAX_BYTES + 1), selection(1, 0), false),
            (
                " ".repeat(MAX_PREFIX_BYTES + 1),
                caret(MAX_PREFIX_BYTES + 1),
                false,
            ),
            ("  x".into(), selection(3, 0), true),
        ] {
            // Avoid native layout of huge buffers or clamping invalid cursors.
            let mut app = app("base", caret(0));
            app.documents[0].text = text.clone();
            app.documents[0].edit_version = if saturated { u64::MAX } else { 7 };
            select(&mut app, selected);
            let version = app.documents[0].edit_version;
            let before_history =
                egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
                    .unwrap()
                    .undoer();
            assert!(handler_frame(&mut app, vec![enter()], true).is_empty());
            assert_eq!(app.documents[0].text, text);
            assert_eq!(app.documents[0].edit_version, version);
            assert_selection(range(&app), selected);
            let after_history =
                egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
                    .unwrap()
                    .undoer();
            assert_eq!(format!("{before_history:?}"), format!("{after_history:?}"));
        }
    }

    #[test]
    fn handler_leaves_ineligible_foreign_focus_and_substantive_batches_untouched() {
        let batches = [
            vec![enter(), egui::Event::Text("x".into())],
            vec![egui::Event::Paste("x".into()), enter()],
            vec![
                enter(),
                egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
            ],
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("字".into())),
                enter(),
            ],
            vec![
                enter(),
                egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
            ],
            vec![
                enter(),
                egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
            ],
            vec![enter(), key(egui::Key::Tab, egui::Modifiers::NONE)],
        ];
        for batch in batches {
            let mut app = app("  abc", selection(5, 2));
            assert_eq!(handler_frame(&mut app, batch.clone(), true), batch);
            assert_eq!(app.documents[0].text, "  abc");
        }
        let mut app = app("  abc", selection(5, 2));
        assert_eq!(handler_frame(&mut app, vec![enter()], false), vec![enter()]);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new("foreign")));
        assert!(
            matches!(handler_frame(&mut app, vec![enter()], true).as_slice(), [event] if plain_enter(event))
        );
        assert_eq!(app.documents[0].text, "  abc");
        assert_eq!(app.documents[0].edit_version, 0);
    }

    #[test]
    fn ime_composition_and_finish_batches_retain_native_semantics() {
        for finish in [
            None,
            Some(egui::ImeEvent::Disabled),
            Some(egui::ImeEvent::Preedit(String::new())),
            Some(egui::ImeEvent::Commit("字".into())),
        ] {
            for enter_first in [false, true] {
                let mut actual = app("  abc", selection(5, 2));
                let mut native = app("  abc", selection(5, 2));
                for app in [&mut actual, &mut native] {
                    frame(
                        app,
                        vec![
                            egui::Event::Ime(egui::ImeEvent::Enabled),
                            egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
                        ],
                    );
                }
                let mut events = vec![enter()];
                if let Some(finish) = finish.clone() {
                    events.insert(usize::from(enter_first), egui::Event::Ime(finish));
                }
                frame(&mut actual, events.clone());
                let ctx = native.editor_ctx.clone();
                let input = raw(&ctx, events);
                let mut frame = eframe::Frame::_new_kittest();
                let _ = ctx.run(input, |ctx| {
                    eframe::App::update(&mut native, ctx, &mut frame)
                });
                // egui processes IME events before key events while enabled.
                assert_eq!(actual.documents[0].text, native.documents[0].text);
                assert_selection(range(&actual), range(&native));
                assert_eq!(
                    actual.documents[0].edit_version,
                    native.documents[0].edit_version
                );
            }
        }
    }

    #[test]
    fn ignored_newline_preedit_and_cancelled_composition_do_not_stick() {
        for newline in ["\n", "\r"] {
            let mut app = app("  x", caret(3));
            frame(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit(newline.into()))],
            );
            frame(&mut app, vec![enter()]);
            assert_eq!(app.documents[0].text, "  x\n  ");
        }
        let mut app = app("  x", caret(3));
        frame(
            &mut app,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
            ],
        );
        let batch = vec![egui::Event::Ime(egui::ImeEvent::Disabled), enter()];
        assert_eq!(handler_frame(&mut app, batch.clone(), false), batch);
        select(&mut app, caret(4));
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, "  x仮\n  ");
    }

    #[test]
    fn prehook_tree_claim_cannot_launder_mixed_enter_or_leak_to_next_frame() {
        let mut app = app("  x", caret(3));
        app.state = crate::ConnectionState::Ready;
        app.explorer.mode = crate::explorer_tree::Mode::Tree;
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(crate::explorer_tree::row_id("")));
        frame(
            &mut app,
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        );
        select(&mut app, caret(3));
        let ctx = app.editor_ctx.clone();
        let mut input = raw(
            &ctx,
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE), enter()],
        );
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        assert_eq!(input.events, vec![enter()]);
        assert!(app.enter_input.is_none());
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        assert_eq!(app.documents[0].text, "  x\n");
        select(&mut app, caret(3));
        frame(&mut app, vec![enter()]);
        assert_eq!(app.documents[0].text, "  x\n  \n");
        assert!(app.enter_input.is_none());
    }

    #[test]
    fn missing_stale_or_wrong_document_frame_grants_leave_enter_native() {
        for stale in [false, true] {
            let mut app = app("  x", caret(3));
            let ctx = app.editor_ctx.clone();
            let mut input = raw(&ctx, vec![enter()]);
            if stale {
                eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
                let _ = ctx.run(raw(&ctx, vec![]), |_| {});
                select(&mut app, caret(3));
            }
            let mut native = eframe::Frame::_new_kittest();
            let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
            assert_eq!(app.documents[0].text, "  x\n");
            assert!(app.enter_input.is_none());
        }
        let mut app = app("  x", caret(3));
        let ctx = app.editor_ctx.clone();
        let input = raw(&ctx, vec![enter()]);
        let mut original = capture(&ctx, &input, app.active());
        app.documents[0].id = 2;
        let id = egui::Id::new(("editor", 2u64));
        let mut state = editor_state::load(&ctx, &mut app.documents[0]);
        state.cursor.set_char_range(Some(caret(3)));
        state.store(&ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
        let _ = ctx.run(input, |ctx| {
            handle(ctx, &mut app.documents[0], true, original.take());
            assert_eq!(ctx.input(|input| input.events.clone()), vec![enter()]);
        });
        assert_eq!(app.documents[0].text, "  x");
    }

    #[test]
    fn focus_loss_and_tab_switch_clear_composition_without_replaying_enter() {
        for tab_switch in [false, true] {
            let mut app = app("  x", caret(3));
            frame(
                &mut app,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
                ],
            );
            if tab_switch {
                app.documents.push(Document::new(
                    2,
                    "second.rs".into(),
                    "other".into(),
                    "r".into(),
                ));
                app.active_document = Some(2);
                app.editor_ctx
                    .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 2u64))));
            } else {
                app.editor_ctx
                    .memory_mut(|memory| memory.request_focus(egui::Id::new("foreign")));
            }
            frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
            app.active_document = Some(1);
            select(&mut app, caret(4));
            frame(&mut app, vec![enter()]);
            assert_eq!(app.documents[0].text, "  x仮\n  ");
        }
    }

    #[test]
    fn actual_forms_find_modal_and_pending_jump_keep_existing_native_behavior() {
        for blocker in 0..6 {
            let mut actual = app("  abc\ndef", selection(5, 2));
            let mut native = app("  abc\ndef", selection(5, 2));
            for app in [&mut actual, &mut native] {
                match blocker {
                    0 => app.open_form = true,
                    1 => app.new_file = true,
                    2 => app.confirm = Some(crate::Confirm::CloseWindow),
                    3 => app.documents[0].jump_to = Some(6),
                    4 => {
                        app.find_open = true;
                        app.find_focus = true;
                    }
                    _ => app.navigation.restore_focus = true,
                }
            }
            frame(&mut actual, vec![enter()]);
            let ctx = native.editor_ctx.clone();
            let mut frame = eframe::Frame::_new_kittest();
            let _ = ctx.run(raw(&ctx, vec![enter()]), |ctx| {
                eframe::App::update(&mut native, ctx, &mut frame)
            });
            assert_eq!(
                actual.documents[0].text, native.documents[0].text,
                "blocker {blocker}"
            );
            assert_eq!(
                actual.documents[0].edit_version, native.documents[0].edit_version,
                "blocker {blocker}"
            );
            assert_selection(range(&actual), range(&native));
        }
    }
}
