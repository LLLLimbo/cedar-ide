//! Copy one captured editor buffer into a fresh, unsaved document. The dialog
//! owns input; only the final frame guard may install its exact snapshot.
use crate::{model::Document, CedarApp, ConnectionState, WorkspaceKey, AMBER, MUTED, RED};
use cedar_protocol::MAX_FILE_BYTES;
use cedar_recovery::WorkspaceIdentity;
use eframe::egui::{self, RichText};
use sha2::{Digest, Sha256};

const MODAL: &str = "copy_draft_modal";
const PATH_INPUT: &str = "copy_draft_path";

struct Source {
    document: u64,
    path: String,
    version: u64,
    text: String,
    baseline: [u8; 32],
    revision: Option<String>,
    interrupted: Option<crate::interrupted_save::InterruptedSave>,
    unverifiable: bool,
    workspace: WorkspaceIdentity,
    key: WorkspaceKey,
    generation: u64,
    connection: ConnectionState,
    navigation: u64,
}

struct Dialog {
    source: Source,
    path: String,
    error: Option<String>,
    confirm: bool,
    focus: bool,
}

#[derive(Default)]
pub(super) struct CopyDraft {
    dialog: Option<Dialog>,
}

impl CopyDraft {
    pub fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn has_cjk(&self) -> bool {
        self.dialog
            .as_ref()
            .is_some_and(|dialog| crate::system_fonts::contains_cjk(&dialog.path))
    }
}

/// Both new-draft entry points accept literal portable paths. In particular,
/// rejecting input must never silently trim it or reinterpret a backslash.
fn draft_path(path: &str) -> Result<(), String> {
    cedar_recovery::validate_relative_path(path).map_err(|error| error.to_string())
}

impl CedarApp {
    /// Allocate only after all admission checks. Zero is reserved, exhaustion
    /// and a live collision fail closed without consuming an identity.
    pub(super) fn allocate_document_id(&mut self) -> Result<u64, String> {
        let id = self.next_document;
        let next = id.checked_add(1).filter(|_| id != 0).ok_or_else(|| {
            "Document identities are exhausted; no new tab was created".to_owned()
        })?;
        if self.documents.iter().any(|doc| doc.id == id) {
            return Err("Document identity is already in use; no new tab was created".into());
        }
        self.next_document = next;
        Ok(id)
    }

    pub(super) fn create_new_file_draft(&mut self) {
        if !self.ready() {
            self.error = Some("Connect a workspace before creating a new file draft".into());
            return;
        }
        if let Err(error) = draft_path(&self.new_path) {
            self.error = Some(error);
            return;
        }
        if self.documents.iter().any(|doc| doc.path == self.new_path) {
            self.error =
                Some("This destination already has an open tab. Choose a different path".into());
            return;
        }
        if self.recovery_workspace().as_ref().is_some_and(|workspace| {
            self.recovery
                .known_path_collision(workspace, &self.new_path)
        }) {
            self.error = Some("A recovery copy is known for this destination. Choose a different path or review that recovery separately".into());
            return;
        }
        if self.documents.len() >= 32 {
            self.error = Some(
                "Close a tab before opening another. Cedar limits the workspace to 32 buffers"
                    .into(),
            );
            return;
        }
        let id = match self.allocate_document_id() {
            Ok(id) => id,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let mut doc = Document::new(id, self.new_path.clone(), String::new(), String::new());
        doc.revision = None;
        self.documents.push(doc);
        self.navigation_changed();
        self.active_document = Some(id);
        self.new_file = false;
    }

    fn copy_draft_transition_problem(&self) -> Option<&'static str> {
        if matches!(
            self.state,
            ConnectionState::Connecting | ConnectionState::Disconnecting
        ) {
            Some("Wait for the connection transition before copying a draft")
        } else if self.save_all_busy() {
            Some("Finish or cancel Save All before copying a draft")
        } else if self.confirm.is_some()
            || self.open_form
            || self.new_file
            || self.close_tab_requested.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.allow_close
            || self.recovery.closing.is_some()
            || self.recovery.pending_restore.is_some()
            || self.recovery.restoring_generation.is_some()
            || self.recovery.remove_confirmation.is_some()
            || self.run_state.transition_pending()
        {
            Some("Finish the current dialog or close transition before copying a draft")
        } else {
            None
        }
    }

    pub(super) fn begin_copy_draft(&mut self) {
        if self.copy_draft.is_open() {
            return;
        }
        let problem = self.copy_draft_transition_problem().or_else(|| {
            if self.navigation.dialog_open() {
                Some("Close the navigation dialog before copying a draft")
            } else {
                None
            }
        });
        if let Some(problem) = problem {
            self.error = Some(problem.into());
            return;
        }
        let Some(doc) = self.active() else {
            self.error = Some("Open an editor buffer to copy its draft".into());
            return;
        };
        if doc.text.len() > MAX_FILE_BYTES {
            self.error = Some("Copy to new draft is limited to 1 MiB of source text".into());
            return;
        }
        if doc.saving {
            self.error = Some("Wait for the current save before copying this draft".into());
            return;
        }
        let (Some(workspace), Some(key)) = (self.recovery_workspace(), self.workspace_key.clone())
        else {
            self.error = Some("The draft's workspace identity is unavailable".into());
            return;
        };
        self.copy_draft.dialog = Some(Dialog {
            source: Source {
                document: doc.id,
                path: doc.path.clone(),
                version: doc.edit_version,
                text: doc.text.clone(),
                baseline: Sha256::digest(doc.saved_text.as_bytes()).into(),
                revision: doc.revision.clone(),
                interrupted: doc.interrupted_save.clone(),
                unverifiable: doc.save_outcome_unverifiable,
                workspace,
                key,
                generation: self.generation,
                connection: self.state,
                navigation: self.navigation_epoch,
            },
            path: String::new(),
            error: None,
            confirm: false,
            focus: true,
        });
        self.navigation.restore_focus = false;
        self.editor_ctx.request_repaint();
    }

    pub(super) fn confirm_copy_draft(&mut self) {
        if let Some(dialog) = &mut self.copy_draft.dialog {
            dialog.confirm = true;
        }
    }

    pub(super) fn dismiss_copy_draft(&mut self) {
        if self.copy_draft.dialog.take().is_some() {
            self.navigation.restore_focus = true;
            self.editor_ctx.request_repaint();
        }
    }

    fn copy_source_current(&self, source: &Source) -> bool {
        self.generation == source.generation
            && self.state == source.connection
            && self.navigation_epoch == source.navigation
            && self.workspace_key.as_ref() == Some(&source.key)
            && self.recovery_workspace().as_ref() == Some(&source.workspace)
            && self.active_document == Some(source.document)
            && self
                .documents
                .iter()
                .filter(|doc| doc.id == source.document)
                .count()
                == 1
            && self.documents.iter().any(|doc| {
                doc.id == source.document
                    && doc.path == source.path
                    && doc.edit_version == source.version
                    && doc.text == source.text
                    && !doc.saving
                    && <[u8; 32]>::from(Sha256::digest(doc.saved_text.as_bytes()))
                        == source.baseline
                    && doc.revision == source.revision
                    && doc.interrupted_save == source.interrupted
                    && doc.save_outcome_unverifiable == source.unverifiable
            })
    }

    /// Runs after every editor, profile, reconciliation and save finisher.
    /// There are no workspace operations or recovery-authority changes here.
    pub(super) fn finish_copy_draft_frame(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.copy_draft.dialog.take() else {
            return;
        };
        if !std::mem::take(&mut dialog.confirm) {
            self.copy_draft.dialog = Some(dialog);
            return;
        }
        let problem = if !self.copy_source_current(&dialog.source) {
            Some("The source draft, selection of tab, or workspace session changed. Cancel and reopen Copy to new draft to capture it again".into())
        } else if ctx.input(|input| input.viewport().close_requested()) {
            Some("Finish the close request before copying a draft".into())
        } else if let Some(problem) = self.copy_draft_transition_problem() {
            Some(problem.into())
        } else if let Err(error) = draft_path(&dialog.path) {
            Some(error)
        } else if self.documents.iter().any(|doc| doc.path == dialog.path) {
            Some("This destination already has an open tab. Choose a different path".into())
        } else if self
            .recovery
            .known_path_collision(&dialog.source.workspace, &dialog.path)
        {
            Some("A recovery copy is known for this destination. Choose a different path or review that recovery separately".into())
        } else if self.documents.len() >= 32 {
            Some("Close a tab before copying another draft (32-buffer limit)".into())
        } else {
            None
        };
        if let Some(error) = problem {
            dialog.error = Some(error);
            self.copy_draft.dialog = Some(dialog);
            ctx.request_repaint();
            return;
        }
        let id = match self.allocate_document_id() {
            Ok(id) => id,
            Err(error) => {
                dialog.error = Some(error);
                self.copy_draft.dialog = Some(dialog);
                ctx.request_repaint();
                return;
            }
        };
        // Construct with an empty baseline rather than treating copied text as
        // a saved acknowledgement. The new ID starts its own native Undo state.
        let mut doc = Document::new(id, dialog.path.clone(), String::new(), String::new());
        doc.revision = None;
        doc.has_cjk |= crate::system_fonts::contains_cjk(&dialog.source.text);
        doc.text = dialog.source.text;
        self.documents.push(doc);
        self.navigation_changed();
        self.active_document = Some(id);
        self.navigation.restore_focus = true;
        self.notice = format!(
            "Copied to {} as an unsaved draft. Use Save separately to create the file; existing files will not be overwritten",
            dialog.path
        );
        ctx.request_repaint();
    }

    pub(super) fn copy_draft_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.copy_draft.dialog.take() else {
            return;
        };
        let mut cancel = false;
        let mut confirm = false;
        let response = egui::Modal::new(egui::Id::new(MODAL)).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 500.0));
            ui.heading("Copy to new draft");
            ui.label(format!("Source: {}", dialog.source.path));
            ui.label("New relative path in this workspace (use forward slashes)");
            let id = egui::Id::new(PATH_INPUT);
            if dialog.focus && !ui.is_sizing_pass() {
                ctx.memory_mut(|memory| memory.request_focus(id));
                dialog.focus = false;
            }
            let input = ui.add(
                egui::TextEdit::singleline(&mut dialog.path)
                    .id(id)
                    .hint_text("src/copied_file.rs")
                    .desired_width(f32::INFINITY),
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "copy_draft_path", &input);
            if input.changed() {
                dialog.error = None;
            }
            ui.label(RichText::new("Copies only this editor's captured text. No workspace file is created until a separate Save. The original tab stays open.").small().color(MUTED));
            if dialog.source.interrupted.is_some() || dialog.source.unverifiable {
                ui.colored_label(AMBER, "The original save outcome remains unknown. Copying does not resolve it; Save All remains blocked.");
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(RED, error);
            }
            ui.horizontal(|ui| {
                let create = ui.button("Create draft");
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "copy_draft_confirm", &create);
                confirm = create.clicked();
                let dismiss = ui.button("Cancel");
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "copy_draft_cancel", &dismiss);
                cancel = dismiss.clicked();
            });
        });
        cancel |= response.should_close();
        // No modal key may become a Save, Find, navigation, or editor action in
        // the opening/closing frame. Native path editing has already consumed it.
        crate::replace::discard_keyboard(ctx);
        self.copy_draft.dialog = Some(dialog);
        if cancel {
            self.dismiss_copy_draft();
        } else if confirm {
            self.confirm_copy_draft();
        }
    }

    #[cfg(test)]
    pub(crate) fn copy_draft_set_path(&mut self, path: &str) {
        self.copy_draft.dialog.as_mut().unwrap().path = path.into();
    }

    #[cfg(test)]
    pub(crate) fn copy_draft_path(&self) -> Option<&str> {
        self.copy_draft
            .dialog
            .as_ref()
            .map(|dialog| dialog.path.as_str())
    }

    #[cfg(test)]
    pub(crate) fn copy_draft_error(&self) -> Option<&str> {
        self.copy_draft
            .dialog
            .as_ref()
            .and_then(|dialog| dialog.error.as_deref())
    }

    #[cfg(test)]
    pub(crate) fn copy_draft_open(&self) -> bool {
        self.copy_draft.is_open()
    }
}

#[cfg(test)]
#[path = "copy_draft_tests.rs"]
mod tests;
