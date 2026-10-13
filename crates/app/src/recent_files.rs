//! Connection-local, bounded path history. Never retains draft text or locations.
use crate::{browse, navigation, CedarApp, WorkspaceKey, MUTED};
use eframe::egui;
use std::{collections::HashSet, sync::Arc};

const LIMIT: usize = 64;
const PATH_BYTES: usize = 256 * 1024;
const QUERY_CHARS: usize = 4096;
const QUERY: &str = "recent_query";

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
            ui.set_width((ctx.screen_rect().width()-64.0).clamp(240.0,560.0));
            ui.heading("Recent files");
            let id = egui::Id::new(QUERY);
            if view.focus && !ui.is_sizing_pass() { ui.memory_mut(|memory| memory.request_focus(id)); view.focus = false; }
            let query = ui.add(egui::TextEdit::singleline(&mut view.query).id(id).char_limit(QUERY_CHARS).desired_width(f32::INFINITY).hint_text("Filter recent paths"));
            #[cfg(test)] crate::workspace_access_tests::record(ui, "recent_query", &query);
            self.recent_response(ui, view.epoch, Target::Query, &query);
            if query.changed() { self.recent_files.input.press = None; self.recent_files.input.click = None; open = None; view.selected = view.rows.iter().find(|row| matches_query(&row.path,&view.query)).map(|row| row.path.clone()); }
            egui::ScrollArea::vertical().id_salt("recent_rows").max_height((ctx.screen_rect().height()-310.0).clamp(75.0,280.0)).show(ui, |ui| {
                let mut count = 0;
                for row in view.rows.iter().filter(|row| matches_query(&row.path,&view.query)) {
                    count += 1;
                    let current = self.recent_row_current(row);
                    let label = format!("{}  ·  {}", row.path, if row.document.is_some() { "Open" } else { "Closed" });
                    let id = egui::Id::new(("recent_row",view.navigation,view.epoch,&row.path,row.document));
                    let response = ui.add_enabled_ui(current, |ui| navigation::candidate_row(ui,id,Some(&row.path)==view.selected.as_ref(),&label,true)).inner;
                    #[cfg(test)] crate::workspace_access_tests::record(ui,&format!("recent_row:{}",row.path),&response);
                    if self.recent_response(ui,view.epoch,Target::Row(row.clone()),&response) { view.selected=Some(row.path.clone()); view.focus=true; }
                }
                if count == 0 { ui.label("No recent files match."); }
            });
            ui.horizontal(|ui| {
                let row = view.rows.iter().find(|row| Some(&row.path)==view.selected.as_ref() && matches_query(&row.path,&view.query)).cloned();
                let label = if row.as_ref().is_some_and(|row| row.document.is_none()) { "Open closed file" } else { "Open" };
                let response = ui.add_enabled(row.as_ref().is_some_and(|row|self.recent_row_current(row)),egui::Button::new(label));
                #[cfg(test)] crate::workspace_access_tests::record(ui,"recent_open",&response);
                if let Some(row) = row { if self.recent_response(ui,view.epoch,Target::Open(row.clone()),&response) { open=Some(row); } }
                let response = ui.add_enabled(!self.recent_files.paths.is_empty(),egui::Button::new("Clear recent"));
                #[cfg(test)] crate::workspace_access_tests::record(ui,"recent_clear",&response);
                clear |= self.recent_response(ui,view.epoch,Target::Clear,&response);
                let response = ui.button("Close");
                #[cfg(test)] crate::workspace_access_tests::record(ui,"recent_close",&response);
                close |= self.recent_response(ui,view.epoch,Target::Close,&response);
            });
            ui.label(egui::RichText::new("This connection only · up to 64 paths · closing a tab keeps its path here. Clear or disconnect removes the list. Closed files use the ordinary Open position; no caret is remembered.").small().color(MUTED));
            if let Some(message)=view.message {ui.label(message);}
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
#[cfg(test)]
mod tests;
