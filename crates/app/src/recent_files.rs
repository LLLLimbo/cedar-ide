//! Connection-local, bounded path history. Never retains draft text or locations.
use crate::{browse, chrome, navigation, CedarApp, WorkspaceKey};
use eframe::egui;
use std::{collections::HashSet, sync::Arc};

const LIMIT: usize = 64;
const PATH_BYTES: usize = 256 * 1024;
const QUERY_CHARS: usize = 4096;
const QUERY: &str = "recent_query";
const PRESENTATION: &str = "recent_presentation";

// One bounded rendering record, never an input/action authorization. The view
// token resets a fixed ScrollArea; the row token survives discarded passes only
// until its one visible reveal. No Response, Context, or file contents are kept.
#[derive(Clone, Default)]
struct Presentation {
    view: Option<egui::Id>,
    reveal: Option<egui::Id>,
}

#[derive(Clone, PartialEq, Eq)]
struct Owner {
    generation: u64,
    workspace: WorkspaceKey,
    root: String,
}
impl Owner {
    fn current(&self, app: &CedarApp) -> bool {
        app.ready()
            && self.generation == app.generation
            && app.workspace_key.as_ref() == Some(&self.workspace)
            && self.root == app.root
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Observed {
    generation: u64,
    document: Option<u64>,
    reopenable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    path: Arc<str>,
    document: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Query,
    Row(Row),
    Open(Row),
    Clear,
    Close,
}
#[derive(Clone)]
struct Presented {
    target: Target,
    id: egui::Id,
    rect: egui::Rect,
    epoch: u64,
}
struct View {
    owner: Owner,
    navigation: u64,
    document: Option<(u64, u64)>,
    epoch: u64,
    rows: Vec<Row>,
    query: String,
    selected: Option<Arc<str>>,
    focus: bool,
    message: Option<&'static str>,
}
#[derive(Default)]
struct Input {
    held: HashSet<egui::Key>,
    claimed: HashSet<egui::Key>,
    composing: bool,
    pass: Option<u64>,
    key: Option<(u64, Target, egui::Id, egui::Key)>,
    presented: Vec<Presented>,
    press: Option<Presented>,
    click: Option<Presented>,
}
#[derive(Default)]
pub(super) struct RecentFiles {
    owner: Option<Owner>,
    paths: Vec<Arc<str>>,
    bytes: usize,
    epoch: u64,
    exhausted: bool,
    observed: Option<Observed>,
    view: Option<View>,
    input: Input,
}
#[cfg(test)]
#[derive(PartialEq, Eq)]
pub(super) struct PreviewHistorySnapshot {
    owner: Option<Owner>,
    paths: Vec<Arc<str>>,
    bytes: usize,
    epoch: u64,
    exhausted: bool,
    observed: Option<Observed>,
}
impl RecentFiles {
    #[cfg(test)]
    pub(super) fn preview_history_snapshot(&self) -> PreviewHistorySnapshot {
        PreviewHistorySnapshot {
            owner: self.owner.clone(),
            paths: self.paths.clone(),
            bytes: self.bytes,
            epoch: self.epoch,
            exhausted: self.exhausted,
            observed: self.observed,
        }
    }

    fn changed(&mut self) -> bool {
        let Some(epoch) = self.epoch.checked_add(1) else {
            self.exhausted = true;
            self.view = None;
            return false;
        };
        self.epoch = epoch;
        self.input.press = None;
        self.input.click = None;
        self.input.key = None;
        true
    }
    fn touch(&mut self, path: &str) {
        if self.exhausted
            || !browse::valid_preview_path(path)
            || self
                .paths
                .first()
                .is_some_and(|first| first.as_ref() == path)
        {
            return;
        }
        if !self.changed() {
            return;
        }
        if let Some(index) = self.paths.iter().position(|old| old.as_ref() == path) {
            self.bytes -= self.paths.remove(index).len();
        }
        while self.paths.len() >= LIMIT
            || self
                .bytes
                .checked_add(path.len())
                .is_none_or(|n| n > PATH_BYTES)
        {
            let Some(old) = self.paths.pop() else {
                return;
            };
            self.bytes -= old.len();
        }
        self.bytes += path.len();
        self.paths.insert(0, Arc::from(path));
    }
    #[cfg(test)]
    pub(super) fn visible_paths(&self) -> Vec<&str> {
        self.view
            .as_ref()
            .map(|v| {
                v.rows
                    .iter()
                    .filter(|r| matches_query(&r.path, &v.query))
                    .map(|r| r.path.as_ref())
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(test)]
    pub(super) fn selected_path(&self) -> Option<&str> {
        self.view.as_ref()?.selected.as_deref()
    }
}
fn matches_query(path: &str, query: &str) -> bool {
    path.to_lowercase().contains(&query.to_lowercase())
}
pub(super) fn shortcut(key: egui::Key, modifiers: egui::Modifiers) -> bool {
    key == egui::Key::E
        && !(modifiers.ctrl && modifiers.mac_cmd)
        && modifiers.matches_exact(egui::Modifiers::COMMAND)
}
fn local_key(target: &Target, key: egui::Key) -> bool {
    key == egui::Key::Escape
        || match target {
            Target::Query | Target::Row(_) => matches!(
                key,
                egui::Key::Enter
                    | egui::Key::ArrowUp
                    | egui::Key::ArrowDown
                    | egui::Key::Home
                    | egui::Key::End
            ),
            _ => matches!(key, egui::Key::Enter | egui::Key::Space),
        }
}
impl CedarApp {
    fn recent_observed(&self) -> Observed {
        Observed {
            generation: self.generation,
            document: self.active_document,
            reopenable: self
                .active()
                .is_some_and(|doc| doc.revision.is_some() && browse::valid_preview_path(&doc.path)),
        }
    }
    pub(super) fn reset_recent_files(&mut self) {
        egui::text_edit::TextEditState::default().store(&self.editor_ctx, egui::Id::new(QUERY));
        // Preserve physical key lifetimes until release. A reconnect must not
        // transform a held shortcut into a new gesture.
        let held = std::mem::take(&mut self.recent_files.input.held);
        let claimed = std::mem::take(&mut self.recent_files.input.claimed);
        let composing = self.recent_files.input.composing;
        self.recent_files = RecentFiles::default();
        self.recent_files.input.held = held;
        self.recent_files.input.claimed = claimed;
        self.recent_files.input.composing = composing;
    }
    pub(super) fn recent_connected(&mut self) {
        self.reset_recent_files();
        self.recent_files.owner = self.workspace_key.clone().map(|workspace| Owner {
            generation: self.generation,
            workspace,
            root: self.root.clone(),
        });
        self.recent_files.observed = Some(self.recent_observed());
    }
    pub(super) fn finish_recent_frame(&mut self, ctx: &egui::Context) {
        if !self.ready() || self.workspace_key.is_none() {
            if self.recent_files.owner.is_some() {
                self.reset_recent_files();
            }
            return;
        }
        if ctx.will_discard() || ctx.output(|output| output.requested_discard()) {
            return;
        }
        if !self
            .recent_files
            .owner
            .as_ref()
            .is_some_and(|owner| owner.current(self))
        {
            self.recent_connected();
            return;
        }
        let observed = self.recent_observed();
        if self.recent_files.observed == Some(observed) {
            return;
        }
        self.recent_files.observed = Some(observed);
        if observed.reopenable {
            let path = self.active().unwrap().path.clone();
            self.recent_files.touch(&path);
        }
    }
    pub(super) fn recent_clear(&mut self) {
        self.recent_files.paths.clear();
        self.recent_files.bytes = 0;
        self.recent_files.observed = Some(self.recent_observed());
        self.recent_files.changed();
        self.recent_retire();
        self.dismiss_navigation();
    }
    pub(super) fn recent_retire(&mut self) {
        self.editor_ctx
            .data_mut(|data| data.remove::<Presentation>(egui::Id::new(PRESENTATION)));
        egui::text_edit::TextEditState::default().store(&self.editor_ctx, egui::Id::new(QUERY));
        self.recent_files.view = None;
        self.recent_files.input.presented.clear();
        self.recent_files.input.press = None;
        self.recent_files.input.click = None;
        self.recent_files.input.key = None;
    }
    pub(super) fn show_recent_files(&mut self) {
        if !self.ready()
            || self.open_form
            || self.new_file
            || self.recent_files.exhausted
            || self.foreign_modal_owns_input(&self.editor_ctx)
        {
            return;
        }
        let Some(owner) = self
            .recent_files
            .owner
            .clone()
            .filter(|owner| owner.current(self))
        else {
            return;
        };
        self.show_recent_marker();
        let mut query_state = egui::text_edit::TextEditState::default();
        query_state.set_undoer(egui::util::undoer::Undoer::with_settings(
            egui::util::undoer::Settings {
                max_undos: crate::editor_state::MAX_UNDO_STATES,
                ..Default::default()
            },
        ));
        query_state.store(&self.editor_ctx, egui::Id::new(QUERY));
        let rows: Vec<_> = self
            .recent_files
            .paths
            .iter()
            .map(|path| Row {
                path: path.clone(),
                document: self
                    .documents
                    .iter()
                    .find(|doc| doc.path == path.as_ref())
                    .map(|doc| doc.id),
            })
            .collect();
        // The most recent other file is the useful default when invoked from
        // an active editor, while the current file remains in the list.
        let selected = rows
            .iter()
            .find(|row| row.document != self.active_document)
            .or_else(|| rows.first())
            .map(|row| row.path.clone());
        self.recent_files.view = Some(View {
            owner,
            navigation: self.navigation_epoch,
            document: self.active().map(|doc| (doc.id, doc.edit_version)),
            epoch: self.recent_files.epoch,
            rows,
            query: String::new(),
            selected,
            focus: true,
            message: None,
        });
    }
    fn recent_view_current(&self, view: &View) -> bool {
        view.owner.current(self)
            && view.navigation == self.navigation_epoch
            && view.document == self.active().map(|doc| (doc.id, doc.edit_version))
            && view.document.is_none_or(|(_, version)| version != u64::MAX)
            && view.epoch == self.recent_files.epoch
            && !self.recent_files.exhausted
    }
    fn recent_row_current(&self, row: &Row) -> bool {
        self.recent_files.paths.iter().any(|path| path == &row.path)
            && self
                .documents
                .iter()
                .find(|doc| doc.path == row.path.as_ref())
                .map(|doc| doc.id)
                == row.document
    }
    pub(super) fn capture_recent_input(
        &mut self,
        ctx: &egui::Context,
        raw: &egui::RawInput,
        consumed: &mut [bool],
    ) {
        let allowed = !self.foreign_modal_owns_input(ctx);
        let view_epoch = self
            .recent_files
            .view
            .as_ref()
            .filter(|view| self.recent_view_current(view))
            .map(|view| view.epoch);
        let focus = ctx.memory(|memory| memory.focused());
        let source = self
            .recent_files
            .input
            .presented
            .iter()
            .find(|item| Some(item.id) == focus && Some(item.epoch) == view_epoch)
            .cloned();
        let input = &mut self.recent_files.input;
        input.pass = Some(ctx.cumulative_pass_nr());
        input.key = None;
        input.click = None;
        if !raw.focused
            || raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
            input.press = None;
            input.held.clear();
            input.claimed.clear();
            input.composing = false;
            return;
        }
        let was_composing = input.composing;
        let clean = (allowed
            && !was_composing
            && raw.focused
            && input.held.is_empty()
            && !ctx.input(|input| input.pointer.any_down() || !input.keys_down.is_empty()))
        .then(|| browse::clean_press(&raw.events))
        .flatten();
        let mut pointer_edges = raw.events.iter().filter_map(|event| match event {
            egui::Event::PointerButton { pressed, .. } => Some(*pressed),
            _ => None,
        });
        let first = pointer_edges.next();
        let second = pointer_edges.next();
        let edges_clean = pointer_edges.next().is_none()
            && !(second.is_some() && (first != Some(true) || second != Some(false)))
            && !(first == Some(true) && input.press.is_some());
        let competing_pointer = !allowed || !raw.focused || was_composing
            || !input.held.is_empty() || !raw.modifiers.is_none()
            || ctx.input(|state| !state.keys_down.is_empty())
            || !edges_clean || !browse::pointer_only(&raw.events)
            || raw.events.iter().any(|event| matches!(event, egui::Event::PointerButton { modifiers, .. } if !modifiers.is_none()));
        if competing_pointer {
            input.press = None;
        }
        // Claim only the adjacent printable companion of an owned Space
        // press/repeat. Release, repress and unrelated input break the fence.
        let mut space_companion = false;
        for (index, event) in raw.events.iter().enumerate() {
            if !matches!(
                event,
                egui::Event::Text(_) | egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_)
            ) {
                space_companion = false;
            }
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => input.composing = true,
                egui::Event::Ime(egui::ImeEvent::Disabled | egui::ImeEvent::Commit(_)) => {
                    input.composing = false
                }
                egui::Event::WindowFocused(false) => {
                    input.press = None;
                    input.held.clear();
                }
                egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } => {
                    let claimed = input.claimed.contains(key);
                    let local = view_epoch.is_some()
                        && modifiers.is_none()
                        && (*key == egui::Key::Escape
                            || source
                                .as_ref()
                                .is_some_and(|item| local_key(&item.target, *key)));
                    if !pressed {
                        input.held.remove(key);
                        input.claimed.remove(key);
                        consumed[index] |= claimed;
                        continue;
                    }
                    let fresh = input.held.insert(*key);
                    if local || claimed {
                        space_companion = *key == egui::Key::Space;
                        consumed[index] = true;
                        if fresh || claimed {
                            input.claimed.insert(*key);
                        }
                        if fresh && !repeat && clean == Some((*key, *modifiers)) {
                            if let Some(item) = &source {
                                input.key = Some((item.epoch, item.target.clone(), item.id, *key));
                            } else if *key == egui::Key::Escape {
                                input.key = view_epoch.map(|epoch| {
                                    (epoch, Target::Close, egui::Id::new(QUERY), *key)
                                });
                            }
                        }
                    }
                }
                egui::Event::Text(text) => {
                    if space_companion && text == " " {
                        consumed[index] = true;
                    }
                    space_companion = false;
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                } if !competing_pointer => {
                    if *pressed {
                        input.press = input
                            .presented
                            .iter()
                            .find(|item| Some(item.epoch) == view_epoch && item.rect.contains(*pos))
                            .cloned();
                    } else if let Some(press) = input.press.take() {
                        if Some(press.epoch) == view_epoch
                            && press.rect.contains(*pos)
                            && input.presented.iter().any(|item| {
                                item.id == press.id
                                    && item.target == press.target
                                    && item.epoch == press.epoch
                                    && item.rect.contains(*pos)
                            })
                        {
                            input.click = Some(press);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn recent_input_current(&self, ctx: &egui::Context) -> bool {
        self.navigation.input_frame_current(ctx)
            && self.recent_files.input.pass
                == Some(
                    ctx.cumulative_pass_nr()
                        .saturating_sub(ctx.output(|out| out.num_completed_passes) as u64),
                )
            && ctx.input(|input| input.focused)
            && !self.foreign_modal_owns_input(ctx)
    }
    fn recent_response(
        &mut self,
        ui: &egui::Ui,
        epoch: u64,
        target: Target,
        response: &egui::Response,
    ) -> bool {
        let clicked = !ui.is_sizing_pass()
            && response.enabled()
            && response.clicked_by(egui::PointerButton::Primary)
            && ui.ctx().interaction_snapshot(|snapshot| snapshot.clicked) == Some(response.id)
            && self.recent_files.input.click.as_ref().is_some_and(|click| {
                click.epoch == epoch && click.id == response.id && click.target == target
            });
        if !ui.is_sizing_pass() && response.enabled() {
            self.recent_files.input.presented.push(Presented {
                target,
                id: response.id,
                rect: response.rect.intersect(ui.clip_rect()),
                epoch,
            });
        }
        clicked
    }
    pub(super) fn recent_window(&mut self, ctx: &egui::Context) {
        let Some(mut view) = self.recent_files.view.take() else {
            self.dismiss_navigation();
            return;
        };
        if !self.recent_view_current(&view) || self.foreign_modal_owns_input(ctx) {
            self.dismiss_navigation();
            navigation::discard_keyboard(ctx);
            return;
        }
        let valid_input = self.recent_input_current(ctx);
        let key = self
            .recent_files
            .input
            .key
            .take()
            .filter(|(epoch, _, id, key)| {
                valid_input
                    && *epoch == view.epoch
                    && (*key == egui::Key::Escape
                        || ctx.memory(|memory| memory.focused()) == Some(*id))
            });
        if !valid_input {
            self.recent_files.input.click = None;
        }
        self.recent_files.input.presented.clear();
        let mut open = None;
        let mut clear = false;
        let mut close = false;
        // A reveal belongs only to this owned selection gesture, never to an
        // idle repaint. It leaves focus in the filter and respects later wheel input.
        let mut reveal = false;
        if let Some((_, target, _, key)) = key {
            if key == egui::Key::Escape {
                close = true;
            } else if matches!(
                key,
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home | egui::Key::End
            ) {
                let rows: Vec<_> = view
                    .rows
                    .iter()
                    .filter(|row| matches_query(&row.path, &view.query))
                    .collect();
                if !rows.is_empty() {
                    let index = rows
                        .iter()
                        .position(|row| Some(&row.path) == view.selected.as_ref())
                        .unwrap_or(0);
                    let index = match key {
                        egui::Key::ArrowUp => index.saturating_sub(1),
                        egui::Key::ArrowDown => (index + 1).min(rows.len() - 1),
                        egui::Key::End => rows.len() - 1,
                        _ => 0,
                    };
                    view.selected = Some(rows[index].path.clone());
                    view.focus = true;
                    reveal = true;
                }
            } else {
                match target {
                    Target::Clear => clear = true,
                    Target::Close => close = true,
                    Target::Open(row) | Target::Row(row) => open = Some(row),
                    Target::Query => {
                        open = view
                            .rows
                            .iter()
                            .find(|row| Some(&row.path) == view.selected.as_ref())
                            .cloned()
                    }
                }
            }
        }
        let modal = egui::Modal::new(egui::Id::new("navigation_modal")).show(ctx, |ui| {
            let width = (ctx.screen_rect().width() - 64.0).clamp(1.0, 640.0);
            ui.set_width(width);
            let top = ui.next_widget_position();
            let gap = 4.0;
            let line = recent_text_size(ui, "Ag", egui::TextStyle::Body).y;
            let button_line = recent_text_size(ui, "Ag", egui::TextStyle::Button).y;
            let row_height = (button_line + 8.0).max(chrome::ROW_HEIGHT);
            let title_height = (line + 4.0).max(24.0);
            let field_height = (line + 2.0 * ui.spacing().button_padding.y)
                .max(ui.spacing().interact_size.y);
            let status_height = 2.0 * line + gap;
            let path_height = line + 6.0;
            let action_height = (button_line + 10.0).max(30.0);
            let fixed_height = title_height + field_height + status_height
                + path_height + action_height + 5.0 * gap;
            let margin = egui::Frame::popup(ui.style()).total_margin().sum().y;
            let height = (fixed_height + 12.0 * row_height)
                .min((ctx.screen_rect().height() - 64.0 - margin).max(1.0));
            let surface = egui::Rect::from_min_size(top, egui::vec2(width, height));
            ui.set_min_size(surface.size());
            let band = |y, h| egui::Rect::from_min_size(egui::pos2(top.x, y), egui::vec2(width, h));
            let title_rect = band(top.y, title_height);
            let connection_width = recent_text_size(ui, "This connection only", egui::TextStyle::Body).x;
            let connection_rect = egui::Rect::from_min_max(
                egui::pos2((title_rect.right() - connection_width).max(title_rect.left()), title_rect.top()),
                title_rect.max,
            );
            recent_label(ui, egui::Rect::from_min_max(title_rect.min, egui::pos2((connection_rect.left() - 12.0).max(title_rect.left()), title_rect.bottom())),
                egui::Label::new(egui::RichText::new("Recent files").strong()).truncate());
            let connection = recent_label(ui, connection_rect,
                egui::Label::new(egui::RichText::new("This connection only").color(chrome::MUTED)).truncate());
            recent_hover(ui, &connection, "Up to 64 recent paths in this connection. Closing a tab keeps its path here; disconnecting clears the list.");
            let id = egui::Id::new(QUERY);
            if view.focus && !ui.is_sizing_pass() { ui.memory_mut(|memory| memory.request_focus(id)); view.focus = false; }
            let query_rect = band(title_rect.bottom() + gap, field_height);
            let query = ui.put(query_rect, egui::TextEdit::singleline(&mut view.query).id(id).char_limit(QUERY_CHARS).desired_width(f32::INFINITY).hint_text("Filter recent paths"));
            recent_hover(ui, &query, "Local path filter; no files are searched.");
            #[cfg(test)] crate::workspace_access_tests::record(ui, "recent_query", &query);
            self.recent_response(ui, view.epoch, Target::Query, &query);
            if query.changed() { self.recent_files.input.press = None; self.recent_files.input.click = None; open = None; view.selected = view.rows.iter().find(|row| matches_query(&row.path,&view.query)).map(|row| row.path.clone()); reveal = true; }
            let presentation_key = egui::Id::new(PRESENTATION);
            let view_id = egui::Id::new(("recent_view", view.owner.generation, view.navigation, view.epoch));
            let selected_id = view.rows.iter().find(|row| Some(&row.path) == view.selected.as_ref())
                .map(|row| egui::Id::new(("recent_row", view.navigation, view.epoch, &row.path, row.document)));
            let mut presentation = ctx.data(|data| data.get_temp::<Presentation>(presentation_key)).unwrap_or_default();
            let reset_scroll = presentation.view != Some(view_id);
            if reset_scroll { presentation = Presentation { view: Some(view_id), reveal: None }; }
            if reveal { presentation.reveal = selected_id; }
            else if presentation.reveal != selected_id || ui.input(|input| input.raw_scroll_delta != egui::Vec2::ZERO || input.pointer.any_pressed()) {
                presentation.reveal = None;
            }
            let visible_pass = !ui.is_sizing_pass() && ui.is_visible() && !ctx.will_discard()
                && !ctx.output(|output| output.requested_discard());
            let mut revealed = false;
            let count = view.rows.iter().filter(|row| matches_query(&row.path, &view.query)).count();
            let status_rect = band(query_rect.bottom() + gap, status_height);
            let headline_rect = band(status_rect.top(), line);
            let headline = if view.message.is_some() {
                "Selection is out of date".to_owned()
            } else {
                recent_count(count, view.rows.len(), view.query.is_empty())
            };
            let headline_response = recent_label(ui, headline_rect,
                egui::Label::new(egui::RichText::new(&headline).color(if view.message.is_some() { crate::AMBER } else { chrome::MUTED })).truncate());
            recent_hover(ui, &headline_response, &headline);
            let details_rect = egui::Rect::from_min_max(egui::pos2(top.x, headline_rect.bottom()), status_rect.max);
            let mut details_ui = ui.new_child(egui::UiBuilder::new().id_salt("recent_status").max_rect(details_rect));
            details_ui.set_clip_rect(ui.clip_rect().intersect(details_rect));
            details_ui.spacing_mut().scroll.dormant_handle_opacity = 0.65;
            let status = egui::ScrollArea::vertical().id_salt(("recent_status_scroll", view.message))
                .animated(false).auto_shrink([false, false]).min_scrolled_height(0.0)
                .max_height(details_rect.height()).show(&mut details_ui, |ui| {
                    ui.add_space(gap / 2.0);
                    ui.label(egui::RichText::new(view.message.unwrap_or("Local path filter · Up to 64 paths"))
                        .color(if view.message.is_some() { crate::AMBER } else { chrome::MUTED }));
                });
            let footer = band(surface.bottom() - action_height, action_height);
            let path_rect = band(footer.top() - gap - path_height, path_height);
            let rows_rect = egui::Rect::from_min_max(egui::pos2(top.x, status_rect.bottom() + gap),
                egui::pos2(surface.right(), (path_rect.top() - gap).max(status_rect.bottom() + gap)));
            let mut rows_ui = ui.new_child(egui::UiBuilder::new().id_salt("recent_rows_panel").max_rect(rows_rect));
            rows_ui.set_clip_rect(ui.clip_rect().intersect(rows_rect));
            rows_ui.spacing_mut().item_spacing.y = 0.0;
            // Keep one persistent ScrollArea ID. A fresh chooser resets it;
            // unique per-view salts would accumulate native scroll states.
            if reset_scroll {
                // Reset native inertia and pending targets as well as offset.
                // Changing only offset lets an interrupted drag move a new view.
                egui::scroll_area::State::default().store(ctx, rows_ui.make_persistent_id(egui::Id::new("recent_rows")));
            }
            let rows_scroll = egui::ScrollArea::vertical().id_salt("recent_rows").animated(false)
                .auto_shrink([false, false]).min_scrolled_height(0.0).max_height(rows_rect.height());
            let rows_scroll = rows_scroll.show(&mut rows_ui, |ui| {
                for row in view.rows.iter().filter(|row| matches_query(&row.path,&view.query)) {
                    let current = self.recent_row_current(row);
                    let id = egui::Id::new(("recent_row",view.navigation,view.epoch,&row.path,row.document));
                    let selected = Some(&row.path) == view.selected.as_ref();
                    let response = ui.add_enabled_ui(current, |ui| recent_row(ui,id,selected,&row.path,row.document.is_some())).inner;
                    #[cfg(test)] crate::workspace_access_tests::record(ui,&format!("recent_row:{}",row.path),&response);
                    if self.recent_response(ui,view.epoch,Target::Row(row.clone()),&response) { view.selected=Some(row.path.clone()); view.focus=true; }
                    if presentation.reveal == Some(id) && selected && visible_pass {
                        response.scroll_to_me(None);
                        revealed = true;
                    }
                }
            });
            if revealed { presentation.reveal = None; }
            ctx.data_mut(|data| data.insert_temp(presentation_key, presentation));
            let row = view.rows.iter().find(|row| Some(&row.path)==view.selected.as_ref() && matches_query(&row.path,&view.query)).cloned();
            let path = row.as_ref().map(|row| row.path.as_ref()).unwrap_or(if view.rows.is_empty() {
                "Files you open appear here"
            } else { "No matching path selected" });
            let path_response = recent_label(ui, path_rect, egui::Label::new(egui::RichText::new(path).color(chrome::MUTED)).truncate());
            recent_hover(ui, &path_response, path);
            ui.painter().hline(surface.x_range(), path_rect.top() - gap / 2.0, egui::Stroke::new(1.0_f32, chrome::BORDER));
            let open_width = recent_text_size(ui, "Open closed file", egui::TextStyle::Button).x + 16.0;
            let clear_width = recent_text_size(ui, "Clear recent", egui::TextStyle::Button).x + 16.0;
            let close_width = recent_text_size(ui, "Close", egui::TextStyle::Button).x + 16.0;
            let open_rect = egui::Rect::from_min_size(footer.min, egui::vec2(open_width, action_height));
            let clear_rect = egui::Rect::from_min_size(egui::pos2(open_rect.right() + gap, footer.top()), egui::vec2(clear_width, action_height));
            let close_rect = egui::Rect::from_min_size(egui::pos2(clear_rect.right() + gap, footer.top()), egui::vec2(close_width, action_height));
            {
                let label = if row.as_ref().is_some_and(|row| row.document.is_none()) { "Open closed file" } else { "Open" };
                let response = recent_button(ui, open_rect, label, row.as_ref().is_some_and(|row|self.recent_row_current(row)));
                recent_hover(ui, &response, "Open the selected path. Closed files use ordinary Open position; no closed caret is remembered.");
                #[cfg(test)] crate::workspace_access_tests::record(ui,"recent_open",&response);
                if let Some(row) = row { if self.recent_response(ui,view.epoch,Target::Open(row.clone()),&response) { open=Some(row); } }
                let response = recent_button(ui, clear_rect, "Clear recent", !self.recent_files.paths.is_empty());
                recent_hover(ui, &response, "Clears this connection’s recent list; keeps files and open tabs.");
                #[cfg(test)] crate::workspace_access_tests::record(ui,"recent_clear",&response);
                clear |= self.recent_response(ui,view.epoch,Target::Clear,&response);
                let response = recent_button(ui, close_rect, "Close", true);
                #[cfg(test)] crate::workspace_access_tests::record(ui,"recent_close",&response);
                close |= self.recent_response(ui,view.epoch,Target::Close,&response);
            }
            let hint = "Enter to open · Esc to close";
            let hint_width = (footer.right() - close_rect.right() - 12.0).max(0.0);
            if recent_text_size(ui, hint, egui::TextStyle::Body).x <= hint_width {
                recent_label(ui, egui::Rect::from_min_max(egui::pos2(close_rect.right() + 12.0, footer.top()), footer.max),
                    egui::Label::new(egui::RichText::new(hint).color(chrome::MUTED)));
            }
            #[cfg(test)] {
                let geometry = tests::Geometry { surface, rows: rows_rect, status: status_rect, headline: headline_rect,
                    details: status.inner_rect, path: path_rect, rows_id: rows_scroll.id, status_id: status.id,
                    rows_content: rows_scroll.content_size, status_content: status.content_size,
                    visible: !ui.is_sizing_pass() && ui.is_visible(), row_height };
                ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("recent_geometry"), geometry));
            }
            #[cfg(not(test))] let _ = (status, rows_scroll);
        });
        close |= modal.backdrop_response.clicked();
        navigation::discard_keyboard(ctx);
        self.recent_files.view = Some(view);
        if clear {
            self.recent_clear();
        } else if close {
            self.dismiss_navigation();
        } else if let Some(row) = open {
            if self.recent_row_current(&row) {
                self.dismiss_navigation();
                self.open(row.path.to_string(), None);
            } else if let Some(view) = &mut self.recent_files.view {
                view.message = Some(
                    "This file changed since it was shown. Reopen Recent files to choose it again.",
                );
            }
        }
    }
}

fn recent_text_size(ui: &egui::Ui, text: &str, style: egui::TextStyle) -> egui::Vec2 {
    egui::WidgetText::from(text)
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
        .size()
}

fn recent_count(count: usize, total: usize, unfiltered: bool) -> String {
    if total == 0 {
        "No recent files in this connection".into()
    } else if count == 0 {
        "No recent files match this filter".into()
    } else if unfiltered {
        format!(
            "{count} recent {}",
            if count == 1 { "file" } else { "files" }
        )
    } else {
        format!(
            "{count} of {total} recent {}",
            if total == 1 { "file" } else { "files" }
        )
    }
}

fn recent_label(ui: &mut egui::Ui, rect: egui::Rect, label: egui::Label) -> egui::Response {
    // A left-origin child also keeps the painted galley left aligned. Ui::put
    // would center the Label widget inside a reserved full-width band.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    let (pos, galley, response) = label.selectable(false).layout_in_ui(&mut child);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, child.is_enabled(), galley.text())
    });
    child
        .painter()
        .galley(pos, galley, child.visuals().text_color());
    response
}

fn recent_button(ui: &mut egui::Ui, rect: egui::Rect, text: &str, enabled: bool) -> egui::Response {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    child.add_enabled(enabled, egui::Button::new(text).min_size(rect.size()))
}

fn recent_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    selected: bool,
    path: &str,
    open: bool,
) -> egui::Response {
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let width = ui.available_width();
    let padding = 6.0;
    let icon_width = 20.0;
    let status = if open { "Open" } else { "Closed" };
    let badge_width = recent_text_size(ui, "Closed", egui::TextStyle::Button).x + 12.0;
    let text_width = (width - 2.0 * padding - icon_width - badge_width - 12.0).max(0.0);
    let name_width = if parent.is_empty() {
        text_width
    } else {
        (text_width * 0.6).min(recent_text_size(ui, name, egui::TextStyle::Button).x)
    };
    let galley = |text: &str, width: f32, color| {
        // Native one-line elision retains the original Unicode layout string.
        // Painted text never supplies a Row identity or an ordinary Open path.
        egui::WidgetText::from(egui::RichText::new(text).color(color)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            width.max(0.0),
            egui::TextStyle::Button,
        )
    };
    let name = galley(name, name_width, chrome::TEXT);
    let parent = galley(
        parent,
        (text_width - name.size().x - 12.0).max(0.0),
        chrome::MUTED,
    );
    let badge = galley(status, badge_width - 12.0, chrome::MUTED);
    let height =
        (name.size().y.max(parent.size().y).max(badge.size().y) + 8.0).max(chrome::ROW_HEIGHT);
    let (_, rect) = ui.allocate_space(egui::vec2(width, height));
    let response = ui.interact(rect, id, egui::Sense::click());
    let accessible = format!("{path} · {status}");
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            &accessible,
        )
    });
    if ui.is_rect_visible(rect) {
        ui.painter().rect(
            rect,
            3.0,
            if selected {
                chrome::SELECTED
            } else if response.hovered() {
                chrome::HOVER
            } else {
                egui::Color32::TRANSPARENT
            },
            if response.has_focus() {
                egui::Stroke::new(1.0_f32, chrome::FOCUS)
            } else {
                egui::Stroke::NONE
            },
            egui::StrokeKind::Inside,
        );
        chrome::paint_icon(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + padding + 8.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            chrome::file_icon(path, false),
            chrome::MUTED,
        );
        let x = rect.left() + padding + icon_width;
        ui.painter().galley(
            egui::pos2(x, rect.center().y - name.size().y / 2.0),
            name.clone(),
            chrome::TEXT,
        );
        ui.painter().galley(
            egui::pos2(
                x + name.size().x + 12.0,
                rect.center().y - parent.size().y / 2.0,
            ),
            parent,
            chrome::MUTED,
        );
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - padding - badge_width,
                rect.center().y - (badge.size().y + 2.0) / 2.0,
            ),
            egui::vec2(badge_width, badge.size().y + 2.0),
        );
        ui.painter().rect_stroke(
            badge_rect,
            3.0,
            egui::Stroke::new(1.0_f32, chrome::BORDER),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            badge_rect.center() - badge.size() / 2.0,
            badge,
            chrome::MUTED,
        );
    }
    let explanation = if !ui.is_enabled() {
        "This file changed since it was shown. Reopen Recent files to choose it again."
    } else if open {
        "Already open; opening reuses this tab."
    } else {
        "Closed; ordinary Open position is used. No closed caret is remembered."
    };
    recent_hover(ui, &response, &format!("{accessible}\n{explanation}"));
    response
}

fn recent_hover(ui: &egui::Ui, response: &egui::Response, text: &str) {
    let hovered = if response.enabled() {
        response.hovered()
    } else {
        response.contains_pointer()
    };
    let visible = response.rect.intersect(ui.clip_rect());
    if !hovered
        || ui.is_sizing_pass()
        || !ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|pos| visible.contains(pos))
        })
    {
        return;
    }
    let (idle, down, still, clicked_since_move) = ui.input(|input| {
        (
            input
                .pointer
                .time_since_last_movement()
                .min(input.pointer.time_since_last_click())
                .min(input.time_since_last_scroll()),
            input.pointer.any_down(),
            input.pointer.is_still() && input.smooth_scroll_delta == egui::Vec2::ZERO,
            input.pointer.time_since_last_click() < input.pointer.time_since_last_movement() + 0.1,
        )
    });
    if down || clicked_since_move {
        return;
    }
    if !still {
        // Keep the native velocity filter progressing even in an otherwise
        // idle GUI; the next stationary pass can start the hover delay.
        ui.ctx().request_repaint();
        return;
    }
    let wait = ui.style().interaction.tooltip_delay - idle;
    if wait > 0.0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(wait));
        return;
    }
    let screen = ui.ctx().screen_rect().shrink(8.0);
    let frame = egui::Frame::popup(ui.style());
    let margin = frame.total_margin().sum();
    let width = (screen.width() - margin.x).clamp(1.0, 480.0);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        width,
        egui::TextStyle::Body,
    );
    let size = egui::vec2(width, galley.size().y) + margin;
    let below = response.rect.bottom() + 4.0;
    let above = response.rect.top() - 4.0 - size.y;
    let y = if below + size.y <= screen.bottom() {
        below
    } else if above >= screen.top() {
        above
    } else {
        below.clamp(screen.top(), (screen.bottom() - size.y).max(screen.top()))
    };
    let x = response
        .rect
        .left()
        .clamp(screen.left(), (screen.right() - size.x).max(screen.left()));
    // Pure presentation: paint the measured galley once, with no Area, Ui,
    // Response, or hit-test layer. Native tooltip Area sizing starts with a
    // 400-point estimate and can leave an invisible hit rectangle over a row.
    // A paint-only tooltip cannot acquire focus or swallow a native press.
    let bounds = egui::Rect::from_min_size(egui::pos2(x, y), size);
    let content = bounds - frame.total_margin();
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            response.id.with("recent_tooltip"),
        ))
        .with_clip_rect(screen);
    painter.add(frame.paint(content));
    painter.galley(content.min, galley, ui.visuals().text_color());
}

#[cfg(test)]
mod tests;
