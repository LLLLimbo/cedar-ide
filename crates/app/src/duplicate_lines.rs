//! Bounded logical-line duplication, owned only by a fresh focused-editor key.
use crate::{editor_state, CedarApp, ConnectionState, WorkspaceKey};
use eframe::egui::{self, text::CCursorRange};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINES: usize = 4096;

pub struct Plan {
    pub text: String,
    pub selection: CCursorRange,
}

/// LF delimits logical lines; CR is otherwise ordinary content. In particular,
/// a cursor between CR and LF is a valid scalar offset that stays in the copy.
pub fn plan(text: &str, selection: CCursorRange) -> Option<Plan> {
    if text.len() > MAX_BYTES {
        return None;
    }
    let low = selection.primary.index.min(selection.secondary.index);
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
    let low_byte = low_byte?;
    let high_byte = high_byte?;
    let start = match text[..low_byte].rfind('\n') {
        Some(byte) => byte.checked_add(1)?,
        None => 0,
    };
    // A selected upper endpoint exactly at a line start excludes that line.
    let end = if low != high && text.as_bytes().get(high_byte.checked_sub(1)?) == Some(&b'\n') {
        high_byte
    } else {
        match text[high_byte..].find('\n') {
            Some(byte) => high_byte.checked_add(byte)?.checked_add(1)?,
            None => text.len(),
        }
    };
    let block = text.get(start..end)?;
    let terminated = block.ends_with('\n');
    let lines = block
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        .checked_add(usize::from(!terminated))?;
    if lines > MAX_LINES {
        return None;
    }
    let separator = if terminated {
        ""
    } else if text[..end]
        .rfind('\n')
        .is_some_and(|lf| lf > 0 && text.as_bytes()[lf - 1] == b'\r')
    {
        "\r\n"
    } else {
        "\n"
    };
    let added = separator.len().checked_add(block.len())?;
    let size = text.len().checked_add(added)?;
    if size > MAX_BYTES {
        return None;
    }
    let shift = block.chars().count().checked_add(separator.len())?;
    let mut mapped = selection;
    mapped.primary.index = mapped.primary.index.checked_add(shift)?;
    mapped.secondary.index = mapped.secondary.index.checked_add(shift)?;
    let mut result = String::with_capacity(size);
    result.push_str(&text[..end]);
    result.push_str(separator);
    result.push_str(block);
    result.push_str(&text[end..]);
    Some(Plan {
        text: result,
        selection: mapped,
    })
}

/// Not Clone: update takes the grant even if the editor is absent or disabled.
pub struct FrameInput {
    pass: u64,
    document: u64,
    path: String,
    version: u64,
    text: String,
    selection: CCursorRange,
    generation: u64,
    connection: ConnectionState,
    session: u64,
    navigation: u64,
    workspace: Option<WorkspaceKey>,
    root: String,
    mac: bool,
}

#[derive(Clone, Copy)]
struct Composition {
    document: u64,
    generation: u64,
    session: u64,
    active: bool,
}

fn composition_id() -> egui::Id {
    egui::Id::new("duplicate_lines_composition")
}

fn same_selection(left: CCursorRange, right: CCursorRange) -> bool {
    left == right
        && left.primary.prefer_next_row == right.primary.prefer_next_row
        && left.secondary.prefer_next_row == right.secondary.prefer_next_row
}

fn modifiers(mac: bool) -> egui::Modifiers {
    egui::Modifiers {
        ctrl: !mac,
        mac_cmd: mac,
        command: true,
        ..egui::Modifiers::NONE
    }
}

fn duplicate_key(event: &egui::Event, mac: bool) -> bool {
    matches!(event, egui::Event::Key {
        key: egui::Key::D, pressed: true, repeat: false, modifiers: actual, ..
    } if *actual == modifiers(mac))
}

fn clean_batch(events: &[egui::Event], mac: bool) -> bool {
    let [press, rest @ ..] = events else {
        return false;
    };
    duplicate_key(press, mac)
        && match rest {
            [] => true,
            [release] => matches!(release, egui::Event::Key {
            key: egui::Key::D, pressed: false, repeat: false, modifiers: actual, ..
        } if *actual == modifiers(mac) || *actual == egui::Modifiers::NONE),
            _ => false,
        }
}

impl CedarApp {
    fn duplicate_eligible(&self, ctx: &egui::Context) -> bool {
        self.editor_indent_eligible(ctx)
            && !self.navigation.coordinate_jump_pending()
            && self.active().is_some_and(|doc| doc.jump_to.is_none())
    }

    /// Must run first in raw_input_hook. Composition is tracked here even when
    /// later navigation or modal handling prevents the editor from being drawn.
    pub(super) fn capture_duplicate_input(
        &self,
        ctx: &egui::Context,
        input: &egui::RawInput,
    ) -> Option<FrameInput> {
        let ime_id = composition_id();
        let Some(doc) = self.active().filter(|doc| {
            input.focused
                && ctx.memory(|memory| memory.has_focus(egui::Id::new(("editor", doc.id))))
        }) else {
            ctx.data_mut(|data| data.remove::<Composition>(ime_id));
            return None;
        };
        let was_composing = ctx.data(|data| {
            data.get_temp::<Composition>(ime_id).is_some_and(|state| {
                state.document == doc.id
                    && state.generation == self.generation
                    && state.session == self.language.session
                    && state.active
            })
        });
        let mut composing = was_composing;
        for event in &input.events {
            match event {
                // Enabled announces IME availability, not active preedit. It
                // still vetoes this entire batch through clean_batch below.
                // Empty preedit clears candidates without ending an IME
                // session. Only its explicit finish events release ownership.
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    composing = false;
                }
                _ => {}
            }
        }
        ctx.data_mut(|data| {
            data.insert_temp(
                ime_id,
                Composition {
                    document: doc.id,
                    generation: self.generation,
                    session: self.language.session,
                    active: composing,
                },
            );
        });
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        if was_composing
            || composing
            || !self.duplicate_eligible(ctx)
            || ctx.input(|input| input.pointer.any_down() || !input.keys_down.is_empty())
            || !clean_batch(&input.events, mac)
            || doc.text.len() > MAX_BYTES
        {
            return None;
        }
        let selection = egui::TextEdit::load_state(ctx, egui::Id::new(("editor", doc.id)))?
            .cursor
            .char_range()?;
        Some(FrameInput {
            pass: ctx.cumulative_pass_nr(),
            document: doc.id,
            path: doc.path.clone(),
            version: doc.edit_version,
            text: doc.text.clone(),
            selection,
            generation: self.generation,
            connection: self.state,
            session: self.language.session,
            navigation: self.navigation_epoch,
            workspace: self.workspace_key.clone(),
            root: self.root.clone(),
            mac,
        })
    }

    pub(super) fn duplicate_lines(
        &mut self,
        ctx: &egui::Context,
        interactive: bool,
        original: Option<FrameInput>,
    ) {
        let Some(original) = original else {
            return;
        };
        if original.pass != ctx.cumulative_pass_nr()
            || !interactive
            || !self.duplicate_eligible(ctx)
            || original.generation != self.generation
            || original.connection != self.state
            || original.session != self.language.session
            || original.navigation != self.navigation_epoch
            || original.workspace != self.workspace_key
            || original.root != self.root
            || original.mac != (ctx.os() == egui::os::OperatingSystem::Mac)
            || !ctx.input(|input| {
                input.focused
                    && !input.pointer.any_down()
                    && input.keys_down.iter().all(|key| *key == egui::Key::D)
                    && clean_batch(&input.raw.events, original.mac)
                    && clean_batch(&input.events, original.mac)
            })
        {
            return;
        }
        let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| Some(doc.id) == self.active_document)
        else {
            return;
        };
        let id = egui::Id::new(("editor", doc.id));
        if original.document != doc.id
            || original.path != doc.path
            || original.version != doc.edit_version
            || original.text != doc.text
            || !ctx.memory(|memory| memory.has_focus(id))
            || !egui::TextEdit::load_state(ctx, id)
                .and_then(|state| state.cursor.char_range())
                .is_some_and(|selection| same_selection(selection, original.selection))
        {
            return;
        }
        // An admitted command that exceeds a bound must remain inert rather
        // than falling through to a native widget shortcut.
        ctx.input_mut(|input| {
            input
                .events
                .retain(|event| !duplicate_key(event, original.mac));
        });
        if doc.edit_version == u64::MAX {
            return;
        }
        if let Some(plan) = plan(&doc.text, original.selection) {
            // egui range equality omits affinity. Refresh the exact Undo anchor
            // before committing the one text-changing native transaction.
            editor_state::move_selection(ctx, doc, original.selection);
            editor_state::commit_selection(ctx, doc, plan.text, plan.selection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;
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

    fn caret(index: usize) -> CCursorRange {
        CCursorRange::one(CCursor::new(index))
    }

    fn assert_selection(actual: CCursorRange, expected: CCursorRange) {
        assert!(
            same_selection(actual, expected),
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn collapsed_unicode_crlf_lone_cr_and_empty_logical_lines() {
        for (source, index, expected, mapped) in [
            ("", 0, "\n", 1),
            ("abc", 1, "abc\nabc", 5),
            ("a\nb", 1, "a\na\nb", 3),
            ("a\nb", 2, "a\nb\nb", 4),
            ("a\r\nb", 2, "a\r\na\r\nb", 5),
            ("a\r\nb", 3, "a\r\nb\r\nb", 6),
            ("a\r\n", 3, "a\r\n\r\n", 5),
            ("\n", 0, "\n\n", 1),
            ("\n", 1, "\n\n", 2),
            ("é😀\r\nβ\nx", 1, "é😀\r\né😀\r\nβ\nx", 5),
            ("a\rb", 2, "a\rb\na\rb", 6),
        ] {
            let result = plan(source, caret(index)).unwrap();
            assert_eq!(result.text, expected, "source {source:?}");
            assert_selection(result.selection, caret(mapped));
        }
    }

    #[test]
    fn selected_whole_lines_exclude_upper_line_start_in_both_directions() {
        let source = "é😀\r\nβ\nlast";
        for (primary, secondary) in [(1, 6), (6, 1)] {
            let before = selection(primary, secondary);
            let result = plan(source, before).unwrap();
            assert_eq!(result.text, "é😀\r\nβ\né😀\r\nβ\nlast");
            assert_selection(result.selection, selection(primary + 6, secondary + 6));
        }
        for (source, before, expected, shift) in [
            ("a\n\nb\n", selection(3, 2), "a\n\n\nb\n", 1),
            ("a\n", selection(2, 0), "a\na\n", 2),
            (
                "é😀\r\nβ\nlast",
                selection(5, 8),
                "é😀\r\nβ\nlast\nβ\nlast",
                7,
            ),
        ] {
            let result = plan(source, before).unwrap();
            assert_eq!(result.text, expected);
            assert_selection(
                result.selection,
                selection(before.primary.index + shift, before.secondary.index + shift),
            );
        }
    }

    #[test]
    fn unterminated_block_uses_nearest_preceding_terminator_verbatim() {
        for (source, before, expected, shift) in [
            ("a\r\nb\nc", selection(6, 1), "a\r\nb\nc\na\r\nb\nc", 7),
            ("a\nb\r\nc", selection(6, 2), "a\nb\r\nc\r\nb\r\nc", 6),
            ("a\r\nβ", selection(4, 3), "a\r\nβ\r\nβ", 3),
            ("a\nb\r\n", selection(5, 5), "a\nb\r\n\r\n", 2),
            ("é\r😀", selection(3, 0), "é\r😀\né\r😀", 4),
        ] {
            let result = plan(source, before).unwrap();
            assert_eq!(result.text, expected, "source {source:?}");
            assert_selection(
                result.selection,
                selection(before.primary.index + shift, before.secondary.index + shift),
            );
        }
    }

    #[test]
    fn every_scalar_endpoint_and_both_affinities_survive_crlf_interiors() {
        let source = "é😀\r\n";
        for primary in 0..=source.chars().count() {
            for secondary in 0..=source.chars().count() {
                for primary_affinity in [false, true] {
                    for secondary_affinity in [false, true] {
                        let mut before = selection(primary, secondary);
                        before.primary.prefer_next_row = primary_affinity;
                        before.secondary.prefer_next_row = secondary_affinity;
                        let result = plan(source, before).unwrap();
                        let shift = if primary == 4 && secondary == 4 { 2 } else { 4 };
                        assert_eq!(
                            result.text,
                            if shift == 2 {
                                "é😀\r\n\r\n"
                            } else {
                                "é😀\r\né😀\r\n"
                            }
                        );
                        let mut expected = before;
                        expected.primary.index += shift;
                        expected.secondary.index += shift;
                        assert_selection(result.selection, expected);
                    }
                }
            }
        }
    }

    #[test]
    fn exact_byte_and_affected_line_limits_and_invalid_offsets_fail_closed() {
        for before in [selection(usize::MAX, 0), selection(4, 0), caret(4)] {
            assert!(plan("abc", before).is_none());
        }
        let exact = format!("a\n{}", "x".repeat(MAX_BYTES - 4));
        assert_eq!(plan(&exact, caret(0)).unwrap().text.len(), MAX_BYTES);
        let too_large = format!("{exact}x");
        assert!(plan(&too_large, caret(0)).is_none());
        let over_input = "x".repeat(MAX_BYTES + 1);
        assert!(plan(&over_input, caret(0)).is_none());
        let lines = "x\n".repeat(MAX_LINES);
        assert!(plan(&lines, selection(lines.len(), 0)).is_some());
        let extra_line = format!("{lines}x");
        assert!(plan(&extra_line, selection(extra_line.len(), 0)).is_none());
        // The limit applies to affected lines, not untouched document lines.
        assert!(plan(&extra_line, caret(0)).is_some());
        assert!(plan(&lines, caret(lines.len())).is_some());
        let eof_fits = "x".repeat((MAX_BYTES - 1) / 2);
        assert!(plan(&eof_fits, caret(0)).unwrap().text.len() <= MAX_BYTES);
        assert!(plan(&format!("{eof_fits}x"), caret(0)).is_none());
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

    fn duplicate(mac: bool) -> egui::Event {
        key(egui::Key::D, modifiers(mac))
    }

    fn release(mac: bool) -> egui::Event {
        let mut event = duplicate(mac);
        if let egui::Event::Key { pressed, .. } = &mut event {
            *pressed = false;
        }
        event
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

    fn frame(app: &mut CedarApp, events: Vec<egui::Event>) {
        let ctx = app.editor_ctx.clone();
        let mut input = raw(&ctx, events);
        eframe::App::raw_input_hook(app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    }

    fn select(app: &mut CedarApp, selected: CCursorRange) {
        let ctx = &app.editor_ctx;
        let id = egui::Id::new(("editor", app.documents[0].id));
        let mut state = editor_state::load(ctx, &mut app.documents[0]);
        state.cursor.set_char_range(Some(selected));
        state.store(ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
    }

    fn app(text: &str, selected: CCursorRange) -> CedarApp {
        let mut app = CedarApp::empty();
        app.open_form = false;
        app.documents.push(Document::new(
            1,
            "main.rs".into(),
            text.into(),
            "revision".into(),
        ));
        app.active_document = Some(1);
        frame(&mut app, vec![]);
        select(&mut app, selected);
        app
    }

    fn range(app: &CedarApp) -> CCursorRange {
        egui::TextEdit::load_state(
            &app.editor_ctx,
            egui::Id::new(("editor", app.documents[0].id)),
        )
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
    }

    fn history(app: &CedarApp) -> String {
        format!(
            "{:?}",
            egui::TextEdit::load_state(
                &app.editor_ctx,
                egui::Id::new(("editor", app.documents[0].id))
            )
            .unwrap()
            .undoer()
        )
    }

    fn handler_frame(
        app: &mut CedarApp,
        events: Vec<egui::Event>,
        interactive: bool,
    ) -> Vec<egui::Event> {
        let ctx = app.editor_ctx.clone();
        let input = raw(&ctx, events);
        let mut grant = app.capture_duplicate_input(&ctx, &input);
        let mut remaining = Vec::new();
        let _ = ctx.run(input, |ctx| {
            app.duplicate_lines(ctx, interactive, grant.take());
            remaining = ctx.input(|input| input.events.clone());
        });
        remaining
    }

    #[test]
    fn strict_platform_modifiers_and_only_one_nonrepeat_key_are_owned() {
        for mac in [false, true] {
            for bits in 0..32 {
                let actual = egui::Modifiers {
                    alt: bits & 1 != 0,
                    ctrl: bits & 2 != 0,
                    shift: bits & 4 != 0,
                    mac_cmd: bits & 8 != 0,
                    command: bits & 16 != 0,
                };
                assert_eq!(
                    clean_batch(&[key(egui::Key::D, actual)], mac),
                    actual == modifiers(mac),
                    "mac={mac}, bits={bits}"
                );
            }
            assert!(clean_batch(&[duplicate(mac), release(mac)], mac));
            assert!(!clean_batch(&[duplicate(mac), duplicate(mac)], mac));
            assert!(!clean_batch(&[release(mac)], mac));
            assert!(!clean_batch(&[release(mac), duplicate(mac)], mac));
            assert!(!clean_batch(
                &[duplicate(mac), release(mac), release(mac)],
                mac
            ));
            let mut repeated = duplicate(mac);
            if let egui::Event::Key { repeat, .. } = &mut repeated {
                *repeat = true;
            }
            assert!(!clean_batch(&[repeated], mac));
        }
    }

    #[test]
    fn actual_frames_unicode_reverse_selection_one_undo_redo_and_saved_state() {
        let source = "é😀\r\nβ\nlast";
        for before in [
            selection(6, 1),
            selection(1, 6),
            selection(3, 2),
            selection(2, 2),
        ] {
            let planned = plan(source, before).unwrap();
            let mut app = app(source, before);
            app.documents[0].saving = true;
            app.documents[0].save_outcome_unverifiable = true;
            frame(&mut app, vec![duplicate(false)]);
            assert_eq!(app.documents[0].text, planned.text);
            assert_selection(range(&app), planned.selection);
            assert_eq!(app.documents[0].edit_version, 1);
            assert!(app.duplicate_input.is_none());
            frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
            assert_eq!(app.documents[0].text, source);
            assert_selection(range(&app), before);
            assert_eq!(app.documents[0].edit_version, 2);
            frame(&mut app, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
            assert_eq!(app.documents[0].text, planned.text);
            assert_selection(range(&app), planned.selection);
            assert_eq!(app.documents[0].edit_version, 3);
            assert_eq!(app.documents[0].saved_text, source);
            assert_eq!(app.documents[0].revision.as_deref(), Some("revision"));
            assert!(app.documents[0].saving);
            assert!(app.documents[0].save_outcome_unverifiable);
        }
    }

    #[test]
    fn simulated_mac_windows_linux_frames_use_the_same_transaction() {
        for (os, mac) in [
            (egui::os::OperatingSystem::Mac, true),
            (egui::os::OperatingSystem::Windows, false),
            (egui::os::OperatingSystem::Nix, false),
        ] {
            let before = selection(3, 2);
            let mut app = app("é😀\r\nlast", before);
            app.editor_ctx.set_os(os);
            frame(&mut app, vec![duplicate(mac), release(mac)]);
            assert_eq!(app.documents[0].text, "é😀\r\né😀\r\nlast");
            assert_selection(range(&app), selection(7, 6));
            assert_eq!(app.documents[0].edit_version, 1);
            frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
            assert_eq!(app.documents[0].text, "é😀\r\nlast");
            assert_selection(range(&app), before);
            assert_eq!(app.documents[0].edit_version, 2);
        }
    }

    #[test]
    fn foreign_modifiers_and_mixed_batches_retain_actual_native_semantics() {
        for (os, mac) in [
            (egui::os::OperatingSystem::Mac, true),
            (egui::os::OperatingSystem::Nix, false),
        ] {
            for events in [
                vec![key(egui::Key::D, egui::Modifiers::CTRL)],
                vec![key(egui::Key::D, modifiers(mac) | egui::Modifiers::SHIFT)],
                vec![duplicate(!mac)],
                vec![duplicate(mac), egui::Event::Text("字".into())],
                vec![egui::Event::Paste("é".into()), duplicate(mac)],
                vec![
                    duplicate(mac),
                    key(egui::Key::ArrowLeft, egui::Modifiers::NONE),
                ],
            ] {
                let before = selection(2, 1);
                let mut actual = app("abc\ndef", before);
                let mut native = app("abc\ndef", before);
                actual.editor_ctx.set_os(os);
                native.editor_ctx.set_os(os);
                frame(&mut actual, events.clone());
                let ctx = native.editor_ctx.clone();
                let mut window = eframe::Frame::_new_kittest();
                let _ = ctx.run(raw(&ctx, events), |ctx| {
                    eframe::App::update(&mut native, ctx, &mut window);
                });
                assert_eq!(actual.documents[0].text, native.documents[0].text);
                assert_eq!(
                    actual.documents[0].edit_version,
                    native.documents[0].edit_version
                );
                assert_selection(range(&actual), range(&native));
            }
        }
    }

    #[test]
    fn same_endpoint_history_affinities_are_replaced_before_the_transaction() {
        let source = "é😀\r\nlast";
        for primary_affinity in [false, true] {
            for secondary_affinity in [false, true] {
                let mut before = selection(3, 2);
                before.primary.prefer_next_row = primary_affinity;
                before.secondary.prefer_next_row = secondary_affinity;
                let mut stale = before;
                stale.primary.prefer_next_row = !primary_affinity;
                stale.secondary.prefer_next_row = !secondary_affinity;
                let mut app = app(source, stale);
                let ctx = app.editor_ctx.clone();
                let mut state = editor_state::load(&ctx, &mut app.documents[0]);
                let mut history = state.undoer();
                history.add_undo(&(stale, source.into()));
                state.set_undoer(history);
                state.cursor.set_char_range(Some(before));
                state.store(&ctx, egui::Id::new(("editor", 1u64)));
                frame(&mut app, vec![duplicate(false)]);
                frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
                assert_eq!(app.documents[0].text, source);
                assert_selection(range(&app), before);
                assert_eq!(app.documents[0].edit_version, 2);
            }
        }
    }

    #[test]
    fn separate_presses_are_distinct_transactions_and_repeat_does_not_edit() {
        let mut app = app("x", caret(1));
        frame(&mut app, vec![duplicate(false)]);
        assert_eq!(app.documents[0].text, "x\nx");
        let mut repeated = duplicate(false);
        if let egui::Event::Key { repeat, .. } = &mut repeated {
            *repeat = true;
        }
        frame(&mut app, vec![repeated]);
        assert_eq!(app.documents[0].edit_version, 1);
        frame(&mut app, vec![release(false)]);
        frame(&mut app, vec![duplicate(false)]);
        assert_eq!(app.documents[0].text, "x\nx\nx");
        assert_eq!(app.documents[0].edit_version, 2);
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, "x\nx");
        frame(&mut app, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        assert_eq!(app.documents[0].text, "x");
        assert_selection(range(&app), caret(1));
    }

    #[test]
    fn admitted_bounds_and_saturated_version_leave_history_and_selection_unchanged() {
        for (source, before, version) in [
            ("abc".into(), selection(usize::MAX, 0), 7),
            ("x".repeat(MAX_BYTES), caret(0), 7),
            (
                "x\n".repeat(MAX_LINES + 1),
                selection((MAX_LINES + 1) * 2, 0),
                7,
            ),
            ("abc".into(), selection(2, 1), u64::MAX),
        ] {
            let mut app = app("base", caret(0));
            app.documents[0].text = source.clone();
            app.documents[0].edit_version = version;
            select(&mut app, before);
            let saved_history = history(&app);
            assert!(handler_frame(&mut app, vec![duplicate(false)], true).is_empty());
            assert_eq!(app.documents[0].text, source);
            assert_eq!(app.documents[0].edit_version, version);
            assert_selection(range(&app), before);
            assert_eq!(history(&app), saved_history);
        }
    }

    #[test]
    fn substantive_mixed_batches_and_all_ime_events_remain_untouched() {
        let mut extras = vec![
            egui::Event::Text("x".into()),
            egui::Event::Paste("pasted".into()),
            key(egui::Key::Tab, egui::Modifiers::NONE),
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            egui::Event::PointerMoved(egui::pos2(10.0, 20.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(10.0, 20.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Disabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("仮".into())),
            egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
            egui::Event::Ime(egui::ImeEvent::Commit("字".into())),
            egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        ];
        let mut released_arrow = key(egui::Key::ArrowDown, egui::Modifiers::NONE);
        if let egui::Event::Key { pressed, .. } = &mut released_arrow {
            *pressed = false;
        }
        extras.push(released_arrow);
        for extra in extras {
            for first in [false, true] {
                let mut events = vec![duplicate(false)];
                events.insert(usize::from(first), extra.clone());
                let mut app = app("abc", selection(2, 1));
                assert_eq!(handler_frame(&mut app, events.clone(), true), events);
                assert_eq!(app.documents[0].text, "abc");
                assert_eq!(app.documents[0].edit_version, 0);
            }
        }
    }

    #[test]
    fn capture_and_execution_both_refuse_forms_modal_focus_loss_and_pending_jump() {
        for block_at_capture in [false, true] {
            for blocker in 0..11 {
                let mut app = app("abc", selection(2, 1));
                let ctx = app.editor_ctx.clone();
                let mut input = raw(&ctx, vec![duplicate(false)]);
                let mut grant = if block_at_capture {
                    None
                } else {
                    app.capture_duplicate_input(&ctx, &input)
                };
                match blocker {
                    0 => app.open_form = true,
                    1 => app.new_file = true,
                    2 => app.confirm = Some(crate::Confirm::CloseWindow),
                    3 => app.documents[0].jump_to = Some(2),
                    4 => app.navigation.restore_focus = true,
                    5 => app.find_focus = true,
                    6 => app.close_tab_requested = Some(1),
                    7 => app.close_after_language_stop = true,
                    8 => app.allow_close = true,
                    9 => ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("foreign"))),
                    _ => input.focused = false,
                }
                if block_at_capture {
                    grant = app.capture_duplicate_input(&ctx, &input);
                    assert!(grant.is_none());
                }
                let _ = ctx.run(input, |ctx| app.duplicate_lines(ctx, true, grant.take()));
                assert_eq!(app.documents[0].text, "abc", "blocker {blocker}");
                assert_eq!(app.documents[0].edit_version, 0);
            }
        }
        let mut app = app("abc", selection(2, 1));
        assert_eq!(
            handler_frame(&mut app, vec![duplicate(false)], false),
            vec![duplicate(false)]
        );
        assert_eq!(app.documents[0].text, "abc");
    }

    #[test]
    fn exact_binding_rejects_changed_selection_affinity_text_document_and_session() {
        for changed in 0..14 {
            let before = selection(2, 1);
            let mut app = app("abc", before);
            let ctx = app.editor_ctx.clone();
            let input = raw(&ctx, vec![duplicate(false)]);
            let mut grant = app.capture_duplicate_input(&ctx, &input);
            assert!(grant.is_some());
            match changed {
                0 => app.documents[0].edit_version += 1,
                1 => app.documents[0].path = "renamed.rs".into(),
                2 => app.documents[0].text = "xyz".into(),
                3 => app.generation += 1,
                4 => app.language.session += 1,
                5 => app.navigation_epoch += 1,
                6 => {
                    app.workspace_key = Some(WorkspaceKey::Local {
                        root: "/other".into(),
                    })
                }
                7 => app.root = "/different".into(),
                8 => select(&mut app, selection(2, 0)),
                9 => {
                    let mut changed = before;
                    changed.primary.prefer_next_row = !before.primary.prefer_next_row;
                    select(&mut app, changed);
                }
                10 => {
                    let mut changed = before;
                    changed.secondary.prefer_next_row = !before.secondary.prefer_next_row;
                    select(&mut app, changed);
                }
                11 => {
                    app.documents[0].id = 2;
                    app.active_document = Some(2);
                    select(&mut app, before);
                }
                12 => app.state = ConnectionState::Disconnected,
                _ => ctx.set_os(egui::os::OperatingSystem::Mac),
            }
            let text = app.documents[0].text.clone();
            let version = app.documents[0].edit_version;
            let saved_history = history(&app);
            let _ = ctx.run(input, |ctx| app.duplicate_lines(ctx, true, grant.take()));
            assert_eq!(app.documents[0].text, text, "change {changed}");
            assert_eq!(app.documents[0].edit_version, version);
            assert_eq!(history(&app), saved_history);
        }
    }

    #[test]
    fn raw_mixed_input_cannot_be_laundered_by_later_filters() {
        let mut app = app("abc", selection(2, 1));
        let ctx = app.editor_ctx.clone();
        let mut input = raw(
            &ctx,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE), duplicate(false)],
        );
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        assert!(app.duplicate_input.is_none());
        input.events.retain(|event| duplicate_key(event, false));
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        assert_eq!(app.documents[0].text, "abc");
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(app.duplicate_input.is_none());
    }

    #[test]
    fn missing_stale_and_undrawn_grants_never_replay() {
        for mode in 0..3 {
            let mut app = app("abc", selection(2, 1));
            let ctx = app.editor_ctx.clone();
            let mut input = raw(&ctx, vec![duplicate(false)]);
            if mode != 0 {
                eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
                assert!(app.duplicate_input.is_some());
                if mode == 1 {
                    let _ = ctx.run(raw(&ctx, vec![]), |_| {});
                    select(&mut app, selection(2, 1));
                } else {
                    app.active_document = None;
                }
            }
            let mut native = eframe::Frame::_new_kittest();
            let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
            assert_eq!(app.documents[0].text, "abc");
            assert_eq!(app.documents[0].edit_version, 0);
            assert!(app.duplicate_input.is_none());
            app.active_document = Some(1);
            select(&mut app, selection(2, 1));
            frame(&mut app, vec![release(false)]);
            frame(&mut app, vec![]);
            assert_eq!(app.documents[0].text, "abc");
            frame(&mut app, vec![duplicate(false)]);
            assert_eq!(app.documents[0].text, "abc\nabc");
            assert_eq!(app.documents[0].edit_version, 1);
        }
    }

    #[test]
    fn composition_spans_frames_and_resets_after_finish_or_focus_loss() {
        for preedit in ["", "\n", "\r"] {
            for reset in 0..3 {
                let mut app = app("abc", caret(1));
                frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
                frame(
                    &mut app,
                    vec![egui::Event::Ime(egui::ImeEvent::Preedit(preedit.into()))],
                );
                let version = app.documents[0].edit_version;
                frame(&mut app, vec![duplicate(false)]);
                assert_eq!(app.documents[0].text, "abc");
                assert_eq!(app.documents[0].edit_version, version);
                frame(&mut app, vec![release(false)]);
                match reset {
                    0 => {
                        frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
                    }
                    1 => {
                        frame(
                            &mut app,
                            vec![egui::Event::Ime(egui::ImeEvent::Commit(String::new()))],
                        );
                    }
                    _ => {
                        app.editor_ctx
                            .memory_mut(|memory| memory.request_focus(egui::Id::new("foreign")));
                        frame(&mut app, vec![]);
                    }
                }
                select(&mut app, caret(1));
                let version = app.documents[0].edit_version;
                frame(&mut app, vec![duplicate(false)]);
                assert_eq!(app.documents[0].text, "abc\nabc");
                assert_eq!(app.documents[0].edit_version, version + 1);
            }
        }
    }

    #[test]
    fn ime_enabled_alone_does_not_block_a_later_clean_shortcut() {
        let mut app = app("abc", caret(1));
        frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
        let version = app.documents[0].edit_version;
        frame(&mut app, vec![duplicate(false)]);
        assert_eq!(app.documents[0].text, "abc\nabc");
        assert_eq!(app.documents[0].edit_version, version + 1);
    }

    #[test]
    fn held_keys_and_pointer_never_grant_a_later_isolated_duplicate() {
        for held in [egui::Key::D, egui::Key::ArrowDown, egui::Key::A] {
            let mut app = app("abc", caret(1));
            let ctx = app.editor_ctx.clone();
            let _ = ctx.run(raw(&ctx, vec![key(held, egui::Modifiers::NONE)]), |_| {});
            select(&mut app, caret(1));
            let input = raw(&ctx, vec![duplicate(false)]);
            assert!(app.capture_duplicate_input(&ctx, &input).is_none());
            let remaining = handler_frame(&mut app, vec![duplicate(false)], true);
            assert!(remaining.iter().any(|event| matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::D,
                    pressed: true,
                    ..
                }
            )));
            assert_eq!(app.documents[0].text, "abc");
            assert_eq!(app.documents[0].edit_version, 0);
        }
        let mut app = app("abc", caret(1));
        let ctx = app.editor_ctx.clone();
        let _ = ctx.run(
            raw(
                &ctx,
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(10.0, 20.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            ),
            |_| {},
        );
        select(&mut app, caret(1));
        assert!(app
            .capture_duplicate_input(&ctx, &raw(&ctx, vec![duplicate(false)]))
            .is_none());
        assert_eq!(
            handler_frame(&mut app, vec![duplicate(false)], true),
            vec![duplicate(false)]
        );
        assert_eq!(app.documents[0].text, "abc");
    }

    #[test]
    fn current_event_filters_cannot_turn_a_granted_key_into_another_command() {
        let mut app = app("abc", caret(1));
        let ctx = app.editor_ctx.clone();
        let input = raw(&ctx, vec![duplicate(false)]);
        let mut grant = app.capture_duplicate_input(&ctx, &input);
        assert!(grant.is_some());
        let _ = ctx.run(input, |ctx| {
            ctx.input_mut(|input| input.events.push(egui::Event::Text("x".into())));
            app.duplicate_lines(ctx, true, grant.take());
            assert_eq!(
                ctx.input(|input| input.events.clone()),
                vec![duplicate(false), egui::Event::Text("x".into())]
            );
        });
        assert_eq!(app.documents[0].text, "abc");
        assert_eq!(app.documents[0].edit_version, 0);
    }
}
