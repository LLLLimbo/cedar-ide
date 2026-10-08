//! Explicit, bounded disk/index reads. Git results never enter editor history.
use crate::{CedarApp, Job, Operation, Payload, AMBER, GREEN, MUTED, RED};
use cedar_protocol::{GitChange, GitChangeKind, GitDiffKind};
use eframe::egui::{self, RichText};
use std::{collections::HashSet, ops::Range};

const MAX_PATH_BYTES: usize = 4096;
const MAX_ENTRIES: usize = 4096;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
// Allow a newer explicit selection while an earlier read finishes, but never
// accumulate an unbounded worker queue from repeated clicks.
const MAX_PENDING_READS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Selection {
    path: String,
    kind: Option<GitDiffKind>,
}

#[derive(Debug)]
enum ReadKind {
    Changes,
    Diff { path: String, kind: GitDiffKind },
}

#[derive(Debug)]
pub(super) struct Action {
    generation: u64,
    epoch: u64,
    program: String,
    kind: ReadKind,
}

struct Patch {
    text: String,
    lines: Vec<Range<usize>>,
}

impl Patch {
    fn new(text: String) -> Self {
        let mut start = 0;
        let lines = text
            .split_inclusive('\n')
            .map(|line| {
                let end = start + line.trim_end_matches('\n').len();
                let range = start..end;
                start += line.len();
                range
            })
            .collect();
        Self { text, lines }
    }

    fn show(&self, ui: &mut egui::Ui) {
        if self.text.is_empty() {
            ui.label(
                "No differences remain for this selection. Refresh status to update the list.",
            );
            return;
        }
        let line_height = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both()
            .id_salt("git_selected_patch")
            .auto_shrink([false, false])
            .show_rows(ui, line_height, self.lines.len(), |ui, visible| {
                for line in &self.lines[visible] {
                    ui.add(
                        egui::Label::new(RichText::new(&self.text[line.clone()]).monospace())
                            .extend(),
                    );
                }
            });
    }
}

#[derive(Default)]
pub(super) struct GitPanel {
    program: String,
    epoch: u64,
    request: Option<u64>,
    entries: Vec<GitChange>,
    status_ready: bool,
    selection: Option<Selection>,
    patch: Option<Patch>,
    message: Option<String>,
}

impl GitPanel {
    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.request = None;
        self.patch = None;
        self.message = None;
    }

    fn clear_results(&mut self) {
        self.invalidate();
        self.entries.clear();
        self.status_ready = false;
        self.selection = None;
    }

    fn current(&self, app: &CedarApp, id: u64, action: &Action) -> bool {
        if !app.typed_git_supported()
            || !app.execution_trusted()
            || action.generation != app.generation
            || action.epoch != self.epoch
            || action.program != self.program
            || self.request != Some(id)
        {
            return false;
        }
        match &action.kind {
            ReadKind::Changes => self.selection.is_none(),
            ReadKind::Diff { path, kind } => self
                .selection
                .as_ref()
                .is_some_and(|selection| &selection.path == path && selection.kind == Some(*kind)),
        }
    }
}

fn path_label(path: &str) -> String {
    // Quotes and escapes make a literal backslash+n distinguishable from a
    // filename containing a newline. This label is never used as an identity.
    format!("{path:?}")
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains('\0')
        && !path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git")
        })
}

fn valid_entries(entries: &[GitChange]) -> bool {
    let mut paths = HashSet::new();
    let mut bytes = 0usize;
    entries.len() <= MAX_ENTRIES
        && entries.iter().all(|entry| {
            bytes = bytes.saturating_add(entry.path.len());
            valid_path(&entry.path)
                && bytes <= MAX_OUTPUT_BYTES
                && paths.insert(entry.path.as_str())
                && ".MADT?!U".contains(entry.index)
                && ".MADT?!U".contains(entry.worktree)
                && (entry.kind == GitChangeKind::File
                    || (!entry.can_diff_staged && !entry.can_diff_unstaged))
        })
}

fn kind_label(kind: GitDiffKind) -> &'static str {
    match kind {
        GitDiffKind::Staged => "Staged · index against HEAD (empty tree before the first commit)",
        GitDiffKind::Unstaged => "Unstaged · disk against index",
    }
}

impl CedarApp {
    pub(super) fn typed_git_supported(&self) -> bool {
        self.backend_supports("git_changes") && self.backend_supports("git_diff")
    }

    fn git_read_slot_available(&self) -> bool {
        self.pending
            .values()
            .filter(|job| matches!(job, Job::GitRead(_)))
            .count()
            < MAX_PENDING_READS
    }

    pub(super) fn reset_git(&mut self, clear_program: bool) {
        self.git_state.clear_results();
        if clear_program {
            self.git_state.program.clear();
        }
        self.git_output = "Refresh to read workspace Git status".into();
    }

    fn set_git_program(&mut self, program: String) {
        if self.git_state.program != program {
            self.cjk_seen |= crate::system_fonts::contains_cjk(&program);
            self.git_state.program = program;
            self.git_state.clear_results();
        }
    }

    fn git_read_problem(&self) -> Option<String> {
        if !self.ready() {
            return Some("Reconnect to the workspace before reading Git changes".into());
        }
        if !self.execution_trusted() {
            return Some("Enable trusted command execution and reconnect. Git may execute repository-configured filters".into());
        }
        if !self.typed_git_supported() {
            return Some(self.unsupported_message("git_changes/git_diff"));
        }
        let program = &self.git_state.program;
        let bytes = program.as_bytes();
        let absolute = program.starts_with('/')
            || program.starts_with("\\\\")
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'));
        if !absolute || program.len() > MAX_PATH_BYTES || program.contains('\0') {
            return Some("Enter an absolute path to Git 2.45 or newer on the workspace host (at most 4096 bytes)".into());
        }
        if !self.git_read_slot_available() {
            return Some("Wait for a pending Git read to finish, then retry this selection".into());
        }
        if self.recovery.closing.is_some() || self.close_after_language_stop {
            return Some(
                "Finish or cancel closing the workspace before reading Git changes".into(),
            );
        }
        None
    }

    fn refresh_git_changes(&mut self) {
        // Invalidate even rejected refreshes: an old success must never replace
        // this attempt's error or masquerade as a fresh status snapshot.
        self.git_state.clear_results();
        if let Some(problem) = self.git_read_problem() {
            self.git_state.message = Some(problem);
            return;
        }
        let program = self.git_state.program.clone();
        let action = Action {
            generation: self.generation,
            epoch: self.git_state.epoch,
            program: program.clone(),
            kind: ReadKind::Changes,
        };
        self.dispatch_git_read(
            Operation::GitChanges {
                git_executable: program,
            },
            action,
        );
    }

    fn select_git_entry(&mut self, path: String) {
        if !self
            .git_state
            .entries
            .iter()
            .any(|entry| entry.path == path)
        {
            return;
        }
        self.git_state.invalidate();
        self.git_state.selection = Some(Selection { path, kind: None });
    }

    fn select_git_diff(&mut self, path: String, kind: GitDiffKind) {
        let eligible = self.git_state.entries.iter().any(|entry| {
            entry.path == path
                && entry.kind == GitChangeKind::File
                && match kind {
                    GitDiffKind::Staged => entry.can_diff_staged,
                    GitDiffKind::Unstaged => entry.can_diff_unstaged,
                }
        });
        self.git_state.invalidate();
        self.git_state.selection = Some(Selection {
            path: path.clone(),
            kind: Some(kind),
        });
        if !eligible {
            self.git_state.message =
                Some("This entry has no supported diff. Refresh status if the file changed".into());
            return;
        }
        if let Some(problem) = self.git_read_problem() {
            self.git_state.message = Some(problem);
            return;
        }
        let program = self.git_state.program.clone();
        let action = Action {
            generation: self.generation,
            epoch: self.git_state.epoch,
            program: program.clone(),
            kind: ReadKind::Diff {
                path: path.clone(),
                kind,
            },
        };
        self.dispatch_git_read(
            Operation::GitDiff {
                git_executable: program,
                path,
                kind,
            },
            action,
        );
    }

    fn dispatch_git_read(&mut self, operation: Operation, action: Action) {
        let id = self.request(operation, Job::GitRead(action));
        if id != 0 {
            self.git_state.request = Some(id);
        } else {
            // request() may have disconnected and cleared the panel. Never
            // leave a spinner or old patch behind after a failed dispatch.
            self.git_state.request = None;
            self.git_state.message = self.error.clone();
        }
    }

    pub(super) fn apply_git_read(
        &mut self,
        id: u64,
        action: Action,
        result: Result<Payload, String>,
    ) {
        let current = self.git_state.current(self, id, &action);
        if self.git_state.request == Some(id) {
            self.git_state.request = None;
        }
        if !current {
            return;
        }
        let problem = match (action.kind, result) {
            (ReadKind::Changes, Ok(Payload::GitChanges { entries })) if valid_entries(&entries) => {
                self.cjk_seen |= entries
                    .iter()
                    .any(|entry| crate::system_fonts::contains_cjk(&entry.path));
                self.git_state.entries = entries;
                self.git_state.status_ready = true;
                None
            }
            (ReadKind::Changes, Ok(Payload::GitChanges { .. })) => Some(
                "Invalid or oversized Git status was ignored. Refresh after checking the agent"
                    .into(),
            ),
            (
                ReadKind::Diff {
                    path: expected_path,
                    kind: expected_kind,
                },
                Ok(Payload::GitDiff { path, kind, text }),
            ) => {
                if path != expected_path || kind != expected_kind {
                    Some(
                        "Git returned a different path or diff kind; the response was ignored"
                            .into(),
                    )
                } else if text.len() > MAX_OUTPUT_BYTES {
                    Some("Git diff exceeded the display limit; no partial patch is shown".into())
                } else {
                    self.cjk_seen |= crate::system_fonts::contains_cjk(&text);
                    self.git_state.patch = Some(Patch::new(text));
                    None
                }
            }
            (_, Err(error)) => Some(error),
            _ => Some("Unexpected Git response; refresh status and retry".into()),
        };
        self.git_state.message = problem;
    }

    pub(super) fn git_panel(&mut self, ui: &mut egui::Ui) {
        if self.ready() && !self.typed_git_supported() {
            self.legacy_git_panel(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Git on workspace host");
            let mut program = self.git_state.program.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut program)
                        .hint_text("Absolute Git executable path")
                        .char_limit(MAX_PATH_BYTES)
                        .desired_width((ui.available_width() - 145.0).max(140.0)),
                )
                .changed()
            {
                self.set_git_program(program);
            }
            let enabled = self.git_read_problem().is_none();
            if ui
                .add_enabled(enabled, egui::Button::new("Refresh status"))
                .clicked()
            {
                self.refresh_git_changes();
            }
        });
        ui.label(RichText::new("Disk and index only; unsaved editor drafts are excluded. Refresh is explicit. Submodules are ignored.").small().color(MUTED));
        ui.label(RichText::new("Git 2.45+ · ordinary repository roots only · global/system Git config is ignored (including global filters and line-ending settings).").small().color(MUTED));
        if !self.execution_trusted() {
            ui.colored_label(AMBER, "Enable trusted command execution and reconnect. Git may run repository filters with your account’s permissions.");
        } else {
            ui.label(RichText::new("Trusted repository filters may execute code, write files, or access the network. These controls only read status and diffs.").small().color(AMBER));
        }
        if let Some(message) = &self.git_state.message {
            ui.colored_label(RED, message);
        }
        if self.git_state.request.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading Git…");
            });
        } else if !self.git_read_slot_available() {
            ui.label("Waiting for earlier Git reads to finish. Then refresh or choose a diff.");
        }
        ui.separator();
        ui.label(
            RichText::new("XY: index / working tree · file names are quoted and escaped")
                .small()
                .color(MUTED),
        );
        let mut selected = None;
        let mut diff = None;
        let mut open = None;
        let can_read = self.git_read_problem().is_none();
        ui.columns(2, |columns| {
            egui::ScrollArea::both().id_salt("git_changes_list").show(&mut columns[0], |ui| {
                if self.git_state.entries.is_empty() {
                    ui.label(if self.git_state.status_ready {
                        "No changes reported (submodules ignored)."
                    } else {
                        "Choose Git on the workspace host, then Refresh status."
                    });
                }
                for entry in &self.git_state.entries {
                    ui.push_id(&entry.path, |ui| {
                        let active = self.git_state.selection.as_ref().is_some_and(|selection| selection.path == entry.path);
                        if ui.selectable_label(active, RichText::new(format!("{}{}  {}", entry.index, entry.worktree, path_label(&entry.path))).monospace()).clicked() {
                            selected = Some(entry.path.clone());
                        }
                        ui.horizontal(|ui| match entry.kind {
                            GitChangeKind::File => {
                                if entry.can_diff_staged && ui.add_enabled(can_read, egui::Button::new("Staged").small()).clicked() {
                                    diff = Some((entry.path.clone(), GitDiffKind::Staged));
                                }
                                if entry.can_diff_unstaged && ui.add_enabled(can_read, egui::Button::new("Unstaged").small()).clicked() {
                                    diff = Some((entry.path.clone(), GitDiffKind::Unstaged));
                                }
                                if !entry.can_diff_staged && !entry.can_diff_unstaged { ui.label("Status only"); }
                            }
                            GitChangeKind::Untracked => {
                                ui.label("Untracked · status only");
                                if ui.add_enabled(self.backend_supports("read"), egui::Button::new("Open file").small()).clicked() { open = Some(entry.path.clone()); }
                            }
                            GitChangeKind::Conflict => { ui.colored_label(AMBER, "Conflict · status only"); }
                            GitChangeKind::Unsupported => { ui.label("Unsupported entry · status only"); }
                        });
                    });
                }
            });
            let ui = &mut columns[1];
            if let Some(selection) = &self.git_state.selection {
                ui.label(RichText::new(path_label(&selection.path)).monospace().color(GREEN));
                if let Some(kind) = selection.kind { ui.label(kind_label(kind)); }
            }
            if let Some(patch) = &self.git_state.patch {
                patch.show(ui);
            } else {
                ui.label(RichText::new("Select Staged or Unstaged to read a file’s diff. No patch is applied to the editor.").small().color(MUTED));
            }
        });
        if let Some(path) = selected {
            self.select_git_entry(path);
        }
        if let Some((path, kind)) = diff {
            self.select_git_diff(path, kind);
        }
        if let Some(path) = open {
            self.open(path, None);
        }
    }

    fn legacy_git_panel(&mut self, ui: &mut egui::Ui) {
        let pending = self.pending.values().any(|job| matches!(job, Job::Git));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.backend_supports("git_status") && self.execution_trusted() && !pending,
                    egui::Button::new("Refresh status"),
                )
                .clicked()
            {
                self.git();
            }
            ui.label(
                RichText::new("Legacy status · upgrade the agent for selected-file diffs")
                    .small()
                    .color(MUTED),
            );
        });
        ui.label(RichText::new("Disk/index status excludes unsaved drafts. Repository filters may execute code; trusted command permission is required.").small().color(AMBER));
        if !self.execution_trusted() {
            ui.colored_label(AMBER, "Enable trusted command execution and reconnect.");
        }
        if !self.backend_supports("git_status") {
            ui.colored_label(
                AMBER,
                self.unsupported_message("git_status or git_changes/git_diff"),
            );
        }
        egui::ScrollArea::both()
            .id_salt("git_output")
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&self.git_output).monospace()).extend());
            });
    }
}

#[cfg(test)]
mod tests;
