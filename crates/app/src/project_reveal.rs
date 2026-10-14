//! An explicit, bounded reveal of the active file. Ordinary Explorer requests
//! retain ownership of their tickets, timeout, validation and cache admission.
use super::*;
use std::time::{Duration, Instant};

pub(super) const PROJECT_SELECTION_COMPLETED: &str = "Selection completed: /";

pub(crate) const MAX_PROJECT_REVEAL_LISTS: usize = 32;
pub(crate) const PROJECT_REVEAL_DEADLINE: Duration = Duration::from_secs(30);

pub(super) struct RevealIntent {
    generation: u64,
    document: u64,
    version: u64,
    path: String,
    navigation: u64,
    mode: Mode,
    mode_epoch: u64,
    epoch: u64,
    directory: String,
    focused: Option<egui::Id>,
    pointer_surrender: bool,
    deadline: Instant,
    requests: usize,
    pending: Option<Ticket>,
    ready: bool,
}

enum Step {
    List(String),
    Ready,
    Missing,
}

impl CedarApp {
    pub(crate) fn project_reveal_pending(&self) -> bool {
        self.explorer.reveal.is_some()
    }

    pub(crate) fn cancel_project_reveal(&mut self) {
        if self.explorer.reveal.take().is_some() {
            self.explorer.message = Some("File selection canceled".into());
        }
        self.explorer.reveal_response = None;
        // Never clear the ordinary outstanding slot. Its eventual reply is
        // eligible only under the existing connection/mode/branch guards.
    }

    pub(crate) fn capture_project_reveal_input(&mut self, input: &egui::RawInput) {
        self.explorer.reveal_response = None;
        // Observe the immutable batch, before any surface consumes input or a
        // ready List reply is polled. Releases, pointer motion and content-free
        // IME lifecycle notifications are inert; even empty composition content
        // cancels. Entry admission still uses the original composition guard.
        if !input.focused
            || input.events.iter().any(|event| {
                actionable(event)
                    && !matches!(
                        event,
                        egui::Event::PointerButton { pressed: false, .. }
                            | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
                    )
            })
        {
            self.cancel_project_reveal();
        }
    }

    pub(crate) fn start_project_reveal(&mut self, ctx: &egui::Context) {
        self.cancel_project_reveal();
        if !self.ready() {
            self.explorer.message =
                Some("Connect a workspace before selecting its current file".into());
            return;
        }
        if self.explorer_busy() {
            self.explorer.message = Some("Project is busy with one directory request. Wait, then select the current file again; no request was queued".into());
            return;
        }
        let Some(doc) = self.active() else {
            self.explorer.message = Some("There is no current file to select in Project".into());
            return;
        };
        if doc.path.is_empty()
            || !valid_path(&doc.path)
            || doc.edit_version == u64::MAX
            || (self.explorer.mode == Mode::Tree && depth(&doc.path) > MAX_DEPTH)
        {
            self.explorer.message = Some("Current file cannot be selected: invalid path, exhausted document identity, or Project path/depth limit (4,096 bytes; Tree depth 32)".into());
            return;
        }
        self.explorer.reveal = Some(RevealIntent {
            generation: self.generation,
            document: doc.id,
            version: doc.edit_version,
            path: doc.path.clone(),
            navigation: self.navigation_epoch,
            mode: self.explorer.mode,
            mode_epoch: self.explorer.mode_epoch,
            epoch: self.explorer.epoch,
            directory: self.directory.clone(),
            focused: ctx.memory(|memory| memory.focused()),
            pointer_surrender: false,
            deadline: Instant::now() + PROJECT_REVEAL_DEADLINE,
            requests: 0,
            pending: None,
            ready: false,
        });
        self.explorer.message = Some("Selecting current file…".into());
        ctx.request_repaint();
    }

    pub(crate) fn project_reveal_allow_pointer_surrender(&mut self, pointer: bool) {
        if let Some(intent) = &mut self.explorer.reveal {
            intent.pointer_surrender = pointer;
        }
    }

    fn project_reveal_identity_current(&self, intent: &RevealIntent) -> bool {
        self.ready()
            && self.generation == intent.generation
            && self.navigation_epoch == intent.navigation
            && self.explorer.mode == intent.mode
            && self.explorer.mode_epoch == intent.mode_epoch
            && self.active().is_some_and(|doc| {
                doc.id == intent.document
                    && doc.edit_version == intent.version
                    && doc.edit_version != u64::MAX
                    && doc.path == intent.path
            })
    }

    fn project_reveal_current(&self, intent: &RevealIntent, ctx: &egui::Context) -> bool {
        self.project_reveal_identity_current(intent)
            && self.explorer.epoch == intent.epoch
            && self.directory == intent.directory
            && ctx.input(|input| input.focused)
            && !self.navigation.blocks_editor()
            && !self.navigation.restore_focus
            && !self.foreign_modal_owns_input(ctx)
            && !self.open_form
            && !self.new_file
            && !self.find_focus
            && self.close_tab_requested.is_none()
            && !self.close_after_language_stop
            && !self.allow_close
            && self.active().is_none_or(|doc| doc.jump_to.is_none())
            && ctx.memory(|memory| {
                memory.focused() == intent.focused
                    || (intent.pointer_surrender && memory.focused().is_none())
            })
    }

    /// Acknowledge only the exact ordinary List ticket after its entire reply
    /// has passed Explorer admission. Do not dispatch continuation from poll.
    pub(super) fn project_reveal_list_settled(&mut self, ticket: &Ticket, admitted: bool) {
        let Some(mut intent) = self.explorer.reveal.take() else {
            return;
        };
        if intent.pending.as_ref() != Some(ticket) {
            self.explorer.reveal = Some(intent);
            return;
        }
        let owned_epoch = if ticket.mode == Mode::Tree {
            intent.epoch.checked_add(1) == Some(self.explorer.epoch)
                && intent.directory == self.directory
        } else {
            intent.epoch == self.explorer.epoch
        };
        if !self.project_reveal_identity_current(&intent)
            || !owned_epoch
            || Instant::now() >= intent.deadline
        {
            self.explorer.message = Some(
                "Select Current File in Project canceled or reached its 30-second deadline".into(),
            );
            return;
        }
        if !admitted {
            self.explorer.message = Some("Current file was not selected: its directory listing failed, was incomplete, or exceeded an Explorer limit. Refresh or Retry explicitly".into());
            return;
        }
        intent.pending = None;
        intent.epoch = self.explorer.epoch;
        intent.directory = self.directory.clone();
        self.explorer.reveal = Some(intent);
    }

    fn project_reveal_step(&self, intent: &RevealIntent) -> Step {
        let parent = parent_path(&intent.path);
        if intent.mode == Mode::Flat {
            let loaded = self
                .explorer
                .flat_admission
                .as_ref()
                .is_some_and(|(_, path)| path == &parent && path == &self.directory)
                && self.explorer.flat_error.is_none();
            return if !loaded {
                Step::List(parent)
            } else if self
                .entries
                .iter()
                .any(|entry| entry.path == intent.path && !entry.is_dir)
            {
                Step::Ready
            } else {
                Step::Missing
            };
        }
        // The target path supplies the only possible ancestor chain. Every
        // next ancestor must also be an exact directory in its accepted parent.
        let mut directory = String::new();
        loop {
            let Some(branch) = self.explorer.branches.get(&directory) else {
                return Step::List(directory);
            };
            let Some(entries) = &branch.entries else {
                return Step::List(directory);
            };
            if branch.stale || branch.error.is_some() {
                return Step::List(directory);
            }
            if directory == parent {
                return if entries
                    .iter()
                    .any(|entry| entry.path == intent.path && !entry.is_dir)
                {
                    Step::Ready
                } else {
                    Step::Missing
                };
            }
            let suffix = if directory.is_empty() {
                parent.as_str()
            } else {
                &parent[directory.len() + 1..]
            };
            let component = suffix
                .split('/')
                .next()
                .expect("a deeper parent has a component");
            let next = if directory.is_empty() {
                component.into()
            } else {
                format!("{directory}/{component}")
            };
            if !entries
                .iter()
                .any(|entry| entry.path == next && entry.is_dir)
            {
                return Step::Missing;
            }
            directory = next;
        }
    }

    /// The caller uses its existing nested scroll reveal. A clipped row can
    /// request scrolling but cannot yet provide proof of visible success.
    pub(crate) fn record_project_reveal_row(
        &mut self,
        ui: &egui::Ui,
        path: &str,
        response: &egui::Response,
    ) -> bool {
        let Some(intent) = &self.explorer.reveal else {
            return false;
        };
        if !intent.ready
            || intent.path != path
            || Instant::now() >= intent.deadline
            || !self.project_reveal_current(intent, ui.ctx())
            || !self.project_focus_row_current(path)
            || !response.enabled()
            || !response.sense.is_focusable()
        {
            return false;
        }
        let viewport = ui.clip_rect().intersect(ui.ctx().screen_rect());
        if !viewport.is_positive() {
            return false;
        }
        let visible = response.rect.intersect(viewport).is_positive();
        self.explorer.reveal_response = Some((response.clone(), visible));
        true
    }

    pub(crate) fn finish_project_reveal_frame(&mut self, ctx: &egui::Context) {
        let response = self.explorer.reveal_response.take();
        if ctx.will_discard() || ctx.output(|output| output.requested_discard()) {
            if self.project_reveal_pending() {
                ctx.request_repaint();
            }
            return;
        }
        let Some(mut intent) = self.explorer.reveal.take() else {
            return;
        };
        if Instant::now() >= intent.deadline {
            self.explorer.message = Some(
                "Current file was not selected: the 30-second Project deadline expired".into(),
            );
            return;
        }
        if !self.project_reveal_current(&intent, ctx) {
            self.explorer.message =
                Some("Select Current File in Project canceled because its context changed".into());
            return;
        }
        ctx.request_repaint_after(intent.deadline.saturating_duration_since(Instant::now()));
        if let Some(ticket) = &intent.pending {
            if self.explorer.outstanding.as_ref() == Some(ticket) {
                self.explorer.reveal = Some(intent);
            } else {
                self.explorer.message = Some(
                    "Current file was not selected: its directory request is no longer current"
                        .into(),
                );
            }
            return;
        }
        match self.project_reveal_step(&intent) {
            Step::Missing => {
                self.explorer.message = Some(format!(
                    "Current file is missing from its complete Project listing: /{}",
                    intent.path
                ));
            }
            Step::Ready => {
                if intent.ready {
                    if let Some((response, true)) = response {
                        if Instant::now() >= intent.deadline {
                            self.explorer.message = Some("Current file was not selected: the 30-second Project deadline expired".into());
                            return;
                        }
                        response.request_focus();
                        self.explorer.message =
                            Some(format!("{PROJECT_SELECTION_COMPLETED}{}", intent.path));
                        self.explorer.selected = intent.path;
                        ctx.request_repaint();
                        return;
                    }
                }
                let needs_repaint = !intent.ready || response.is_some();
                intent.ready = true;
                self.explorer.reveal = Some(intent);
                if needs_repaint {
                    ctx.request_repaint();
                }
            }
            Step::List(path) => {
                let limit = if intent.mode == Mode::Flat {
                    1
                } else {
                    MAX_PROJECT_REVEAL_LISTS
                };
                if intent.requests >= limit {
                    self.explorer.message = Some(format!("Current file was not selected: Project reached its {limit}-directory request limit"));
                    return;
                }
                if self.explorer_busy() {
                    self.explorer.message = Some(
                        "Current file was not selected: Project is busy; no request was queued"
                            .into(),
                    );
                    return;
                }
                // State is taken out while using the ordinary manual helpers,
                // so only this owned transition avoids their cancellation hook.
                if Instant::now() >= intent.deadline {
                    self.explorer.message = Some(
                        "Current file was not selected: the 30-second Project deadline expired"
                            .into(),
                    );
                    return;
                }
                if intent.mode == Mode::Flat {
                    self.explorer_list_flat(path.clone());
                } else {
                    self.explorer_refresh(&path);
                }
                let Some(ticket) = self
                    .explorer
                    .outstanding
                    .as_ref()
                    .filter(|ticket| {
                        ticket.path == path
                            && ticket.mode == intent.mode
                            && ticket.generation == intent.generation
                            && ticket.mode_epoch == intent.mode_epoch
                    })
                    .cloned()
                else {
                    if self.explorer.message.is_none() {
                        self.explorer.message = Some(
                            "Current file was not selected: its directory request could not start"
                                .into(),
                        );
                    }
                    return;
                };
                intent.requests += 1;
                intent.pending = Some(ticket);
                intent.epoch = self.explorer.epoch;
                self.explorer.message = Some("Selecting current file…".into());
                self.explorer.reveal = Some(intent);
            }
        }
    }
}

#[cfg(test)]
#[path = "project_reveal_tests.rs"]
mod tests;
