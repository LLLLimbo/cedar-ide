//! Bounded disk review. Explicit reload and draft merge both verify a fresh Read
//! and commit only at the native frame's final input barrier.
use crate::{editor_state, CedarApp, Job, Operation, Payload, GREEN, MUTED};
use cedar_protocol::MAX_FILE_BYTES;
use eframe::egui::{self, RichText};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    Compare,
    VerifyReload,
    VerifyMerge,
}

#[derive(Debug)]
struct Source {
    generation: u64,
    navigation: u64,
    document: u64,
    path: String,
    edit_version: u64,
    revision: Option<String>,
    profile_epoch: u64,
}

impl Source {
    fn capture(app: &CedarApp) -> Option<Self> {
        let doc = app.active()?;
        // No unbounded draft or path is retained in a request or review token.
        if !valid_path(&doc.path) || doc.revision.as_ref().is_some_and(|rev| rev.len() > 64) {
            return None;
        }
        Some(Self {
            generation: app.generation,
            navigation: app.navigation_epoch,
            document: doc.id,
            path: doc.path.clone(),
            edit_version: doc.edit_version,
            revision: doc.revision.clone(),
            profile_epoch: app.profiles.epoch,
        })
    }

    fn same_document(&self, app: &CedarApp) -> bool {
        app.backend_supports("read")
            && self.generation == app.generation
            && self.navigation == app.navigation_epoch
            && app
                .active()
                .is_some_and(|doc| doc.id == self.document && doc.path == self.path)
    }

    fn unchanged(&self, app: &CedarApp) -> bool {
        self.same_document(app)
            && app.active().is_some_and(|doc| {
                doc.edit_version == self.edit_version && doc.revision == self.revision
            })
    }
}

struct Snapshot {
    text: String,
    revision: String,
}

struct MergePreview {
    base: String,
    draft: String,
    selection: Option<egui::text::CCursorRange>,
    text: String,
}

fn native_selection(ctx: &egui::Context, document: u64) -> Option<egui::text::CCursorRange> {
    // Preview must not initialize Undo or store an editor state.
    egui::TextEdit::load_state(ctx, egui::Id::new(("editor", document)))
        .and_then(|state| state.cursor.char_range())
}

fn same_selection(
    left: Option<egui::text::CCursorRange>,
    right: Option<egui::text::CCursorRange>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            // CCursor's PartialEq intentionally ignores wrapped-row affinity.
            left.primary.index == right.primary.index
                && left.primary.prefer_next_row == right.primary.prefer_next_row
                && left.secondary.index == right.secondary.index
                && left.secondary.prefer_next_row == right.secondary.prefer_next_row
        }
        _ => false,
    }
}

fn bounded_merge_text(text: &str) -> bool {
    text.len() <= MAX_FILE_BYTES
        && !text.contains('\0')
        && text.split_inclusive('\n').take(65_537).count() <= 65_536
}

fn content_revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[derive(Default)]
struct PaneLayout {
    version: u64,
    pixels_per_point: f32,
    galley: Option<Arc<egui::Galley>>,
    atlas: Option<Arc<egui::epaint::mutex::Mutex<egui::epaint::TextureAtlas>>>,
}

impl PaneLayout {
    fn show(&mut self, ui: &mut egui::Ui, text: &str, version: u64) {
        let pixels_per_point = ui.ctx().pixels_per_point();
        let atlas = ui.fonts(|fonts| fonts.texture_atlas());
        if self.galley.is_none()
            || self.version != version
            || self.pixels_per_point != pixels_per_point
            || self
                .atlas
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &atlas))
        {
            self.version = version;
            self.pixels_per_point = pixels_per_point;
            // Hold the actual atlas identity, not an address that can be reused.
            // set_fonts activates next pass; atlas resets also rebuild UVs.
            self.atlas = Some(atlas);
            self.galley = Some(ui.fonts(|fonts| {
                fonts.layout_no_wrap(text.to_owned(), egui::FontId::monospace(13.0), crate::TEXT)
            }));
        }
        // TextEdit clones its entire input even with interactive(false). Cached
        // labels avoid that clone and have no editor state or undo history.
        ui.add(
            egui::Label::new(self.galley.as_ref().unwrap().clone())
                .selectable(false)
                .extend(),
        );
    }
}

struct Review {
    source: Source,
    snapshot: Option<Snapshot>,
    pending: Option<Purpose>,
    staged: bool,
    merge: Option<MergePreview>,
    merge_staged: bool,
    message: Option<String>,
    draft_layout: PaneLayout,
    disk_layout: PaneLayout,
    merge_layout: PaneLayout,
}

#[derive(Default)]
pub(super) struct DiskReview {
    epoch: u64,
    slot: Option<Review>,
    // Dismissal releases the visible text, but cannot cancel the wire Read.
    // Keep at most one outstanding read until its reply drains or reconnect.
    pub outstanding: Option<u64>,
}

impl DiskReview {
    pub fn busy(&self) -> bool {
        self.outstanding.is_some()
            || self.slot.as_ref().is_some_and(|review| {
                review.pending.is_some() || review.staged || review.merge_staged
            })
    }
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.contains(['\0', '\\', ':'])
        && !path.starts_with('/')
        && !path.split('/').any(|part| part == "..")
}

fn validate_snapshot(source: &Source, payload: Payload) -> Result<Snapshot, &'static str> {
    let Payload::File {
        path,
        text,
        revision,
    } = payload
    else {
        return Err("Unexpected disk response; the editor is unchanged");
    };
    if path != source.path || !valid_path(&path) {
        return Err("The agent returned a different or invalid path; the response was ignored");
    }
    if text.len() > MAX_FILE_BYTES || text.contains('\0') {
        return Err(
            "The disk response is oversized or contains NUL bytes; the editor is unchanged",
        );
    }
    // The existing Read contract returns a canonical SHA-256 revision. Treat it
    // as a server concurrency token, never as proof of the remote file's identity.
    if revision.len() != 64
        || !revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("The disk response has an invalid revision; the editor is unchanged");
    }
    Ok(Snapshot { text, revision })
}

impl CedarApp {
    pub(super) fn dismiss_disk_review(&mut self) {
        self.disk_review.epoch = self.disk_review.epoch.wrapping_add(1);
        self.disk_review.slot = None;
        // The one outstanding ticket-only job remains until its response. It
        // keeps repeated dismiss/reopen clicks from growing the worker queue.
    }

    pub(super) fn compare_with_disk(&mut self) {
        if self.disk_review.busy() || self.interrupted_save_check.busy() {
            return;
        }
        if !self.backend_supports("read") {
            self.error =
                Some("Reconnect to a Read-capable workspace before comparing with disk".into());
            return;
        }
        let Some(source) = Source::capture(self) else {
            self.error =
                Some("Open a file with a valid workspace path before comparing with disk".into());
            return;
        };
        let path = source.path.clone();
        self.dismiss_disk_review();
        self.disk_review.slot = Some(Review {
            source,
            snapshot: None,
            pending: Some(Purpose::Compare),
            staged: false,
            merge: None,
            merge_staged: false,
            message: None,
            draft_layout: PaneLayout::default(),
            disk_layout: PaneLayout::default(),
            merge_layout: PaneLayout::default(),
        });
        self.request_disk_read(path, Purpose::Compare);
    }

    fn request_disk_read(&mut self, path: String, purpose: Purpose) {
        let ticket = self.disk_review.epoch;
        let id = self.request(
            Operation::Read { path },
            Job::DiskReview { ticket, purpose },
        );
        if id == 0 {
            if let Some(review) = &mut self.disk_review.slot {
                review.pending = None;
                if purpose == Purpose::VerifyMerge {
                    review.merge = None;
                    review.merge_layout = PaneLayout::default();
                }
                review.message = Some("Disk read could not start; reconnect and refresh".into());
            }
        } else {
            self.disk_review.outstanding = Some(id);
        }
    }

    fn reload_source_problem(&self, source: &Source, check_form: bool) -> Option<&'static str> {
        if !source.unchanged(self) {
            return Some(
                "The tab or its baseline changed. Refresh the comparison before reloading",
            );
        }
        let doc = self.active().unwrap();
        if doc.dirty() {
            return Some(
                "Reload needs a clean tab. Preview merge or copy your draft to preserve unsaved work",
            );
        }
        if doc.saving
            || self
                .pending
                .values()
                .any(|job| matches!(job, Job::Save { document, .. } if *document == doc.id))
        {
            return Some("Wait for this file's save before reloading");
        }
        if self.close_tab_requested.is_some()
            || self.confirm.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.recovery.closing.is_some()
            || self.allow_close
        {
            return Some("Finish or cancel the close action before reloading");
        }
        if check_form
            && source.path == crate::profile_ui::PATH
            && source.profile_epoch != self.profiles.epoch
        {
            return Some(
                "The profile form changed during the disk read. Review it and reload again",
            );
        }
        None
    }

    pub(super) fn reload_from_disk(&mut self) {
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        if self.disk_review.busy() || self.interrupted_save_check.busy() {
            return;
        }
        let problem = if review.snapshot.is_none() {
            Some("Read and review a disk snapshot before reloading")
        } else {
            self.reload_source_problem(&review.source, false)
        };
        if let Some(problem) = problem {
            self.disk_review.slot.as_mut().unwrap().message = Some(problem.into());
            return;
        }
        self.disk_review.epoch = self.disk_review.epoch.wrapping_add(1);
        let review = self.disk_review.slot.as_mut().unwrap();
        review.source.profile_epoch = self.profiles.epoch;
        review.pending = Some(Purpose::VerifyReload);
        review.message = None;
        let path = review.source.path.clone();
        self.request_disk_read(path, Purpose::VerifyReload);
    }

    fn merge_source_problem(&self, source: &Source, ctx: &egui::Context) -> Option<&'static str> {
        if !source.unchanged(self) {
            return Some("The tab or baseline changed. Refresh the comparison and preview again");
        }
        let doc = self.active().unwrap();
        if doc.revision.is_none() || doc.interrupted_save.is_some() {
            return Some("Merge needs a known saved baseline. Resolve any interrupted save first");
        }
        if doc.text == doc.saved_text {
            return Some("Merge needs unsaved draft changes; use Reload clean tab for a clean tab");
        }
        if self.documents.iter().any(|doc| doc.saving) || self.mutation_pending() {
            return Some("Wait for pending save or workspace changes before previewing a merge");
        }
        if self.interrupted_save_check.busy() {
            return Some("Finish the interrupted-save check before previewing a merge");
        }
        if self.close_tab_requested.is_some()
            || self.confirm.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.recovery.closing.is_some()
            || self.allow_close
            || self.foreign_modal_owns_input(ctx)
            || self.navigation.blocks_editor()
            || self.open_form
            || self.new_file
            || self.recovery.pending_restore.is_some()
            || doc.jump_to.is_some()
        {
            return Some("Finish or cancel the dialog, navigation, or close action before merging");
        }
        if source.path == crate::profile_ui::PATH && source.profile_epoch != self.profiles.epoch {
            return Some("The profile form changed. Review the form and preview the merge again");
        }
        None
    }

    fn merge_preview_problem(&self, review: &Review, ctx: &egui::Context) -> Option<&'static str> {
        if let Some(problem) = self.merge_source_problem(&review.source, ctx) {
            return Some(problem);
        }
        let Some(preview) = &review.merge else {
            return Some("Preview a merge before applying it");
        };
        let doc = self.active().unwrap();
        if doc.saved_text != preview.base
            || doc.text != preview.draft
            || !same_selection(preview.selection, native_selection(ctx, doc.id))
        {
            return Some(
                "The draft, saved baseline, or selection changed. Preview the merge again",
            );
        }
        None
    }

    fn clear_merge_preview(&mut self, message: Option<String>) {
        if let Some(review) = &mut self.disk_review.slot {
            review.merge = None;
            review.merge_staged = false;
            review.merge_layout = PaneLayout::default();
            if review.pending == Some(Purpose::VerifyMerge) {
                self.disk_review.epoch = self.disk_review.epoch.wrapping_add(1);
                review.pending = None;
            }
            review.message = message;
        }
    }

    pub(super) fn cancel_disk_merge(&mut self) {
        self.clear_merge_preview(None);
    }

    pub(super) fn preview_disk_merge(&mut self, ctx: &egui::Context) {
        if self.disk_review.busy() {
            return;
        }
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        // A new explicit preview may capture newer profile form input, but never
        // a changed editor or baseline under the earlier Compare request.
        let Some(mut source) = Source::capture(self) else {
            return;
        };
        source.profile_epoch = self.profiles.epoch;
        let result = (|| {
            if !review.source.unchanged(self) {
                return Err(
                    "The draft changed. Refresh the comparison before previewing a merge".into(),
                );
            }
            if let Some(problem) = self.merge_source_problem(&source, ctx) {
                return Err(problem.into());
            }
            let snapshot = review
                .snapshot
                .as_ref()
                .ok_or("Read a disk snapshot before previewing a merge")?;
            let doc = self.active().unwrap();
            if ![
                doc.saved_text.as_str(),
                doc.text.as_str(),
                snapshot.text.as_str(),
            ]
            .into_iter()
            .all(bounded_merge_text)
            {
                return Err(
                    "Merge inputs must each fit within 1 MiB and 65,536 lines, without NUL bytes"
                        .into(),
                );
            }
            if doc.revision.as_ref() != Some(&content_revision(&doc.saved_text)) {
                return Err(
                    "The saved baseline does not match its SHA-256 revision. Merge is unavailable"
                        .into(),
                );
            }
            if snapshot.revision != content_revision(&snapshot.text) {
                return Err(
                    "Disk contents do not match their SHA-256 revision. Refresh the comparison"
                        .into(),
                );
            }
            let selection = native_selection(ctx, doc.id);
            if selection.is_some_and(|range| {
                let chars = doc.text.chars().count();
                range.primary.index > chars || range.secondary.index > chars
            }) {
                return Err(
                    "The selection is outside the draft. Select a valid position and preview again"
                        .into(),
                );
            }
            let text = crate::disk_merge::merge_separate_changes(
                &doc.saved_text,
                &doc.text,
                &snapshot.text,
            )
            .map_err(|error| {
                format!("{error}. Keep or copy your draft and review the differences")
            })?;
            if text == doc.text || text == snapshot.text {
                return Err("There are no separate draft and disk changes to merge".into());
            }
            Ok(MergePreview {
                base: doc.saved_text.clone(),
                draft: doc.text.clone(),
                selection,
                text,
            })
        })();
        self.clear_merge_preview(None);
        let review = self.disk_review.slot.as_mut().unwrap();
        match result {
            Ok(preview) => {
                review.source = source;
                review.merge = Some(preview);
                review.message = Some("Merge preview only. Apply rechecks disk, changes the draft as one Undo, and keeps it unsaved. Save separately".into());
            }
            Err(message) => review.message = Some(message),
        }
    }

    pub(super) fn apply_disk_merge(&mut self, ctx: &egui::Context) {
        if self.disk_review.busy() {
            return;
        }
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        if let Some(problem) = self.merge_preview_problem(review, ctx) {
            self.clear_merge_preview(Some(problem.into()));
            return;
        }
        self.disk_review.epoch = self.disk_review.epoch.wrapping_add(1);
        let review = self.disk_review.slot.as_mut().unwrap();
        review.pending = Some(Purpose::VerifyMerge);
        review.message = Some("Rechecking disk before applying the preview…".into());
        let path = review.source.path.clone();
        self.request_disk_read(path, Purpose::VerifyMerge);
    }

    pub(super) fn apply_disk_read(
        &mut self,
        ticket: u64,
        purpose: Purpose,
        result: Result<Payload, String>,
        connected: bool,
    ) {
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        if ticket != self.disk_review.epoch || review.pending != Some(purpose) {
            return;
        }
        // Check freshness before even displaying errors. Stale transport errors
        // must not close a newer review or replace a newer connection's status.
        if !review.source.unchanged(self) {
            if !review.source.same_document(self) {
                self.dismiss_disk_review();
            } else {
                let review = self.disk_review.slot.as_mut().unwrap();
                review.pending = None;
                review.merge = None;
                review.merge_staged = false;
                review.merge_layout = PaneLayout::default();
                review.message =
                    Some("The draft changed while reading disk. Refresh to compare again".into());
            }
            return;
        }
        if purpose == Purpose::VerifyMerge {
            if let Some(problem) = self.merge_preview_problem(review, &self.editor_ctx) {
                self.clear_merge_preview(Some(problem.into()));
                return;
            }
        }
        if purpose == Purpose::VerifyReload {
            if let Some(problem) = self.reload_source_problem(&review.source, true) {
                let review = self.disk_review.slot.as_mut().unwrap();
                review.pending = None;
                review.message = Some(problem.into());
                return;
            }
        }
        let snapshot = match result {
            Ok(payload) => validate_snapshot(&review.source, payload).map_err(str::to_owned),
            Err(error) => {
                if !connected {
                    self.disconnected(error);
                    return;
                }
                // Bound displayed remote errors, too; never retain arbitrary payloads.
                Err(error.chars().take(2048).collect())
            }
        };
        let review = self.disk_review.slot.as_mut().unwrap();
        review.pending = None;
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                review.message = Some(error);
                review.snapshot = None;
                review.disk_layout = PaneLayout::default();
                review.merge = None;
                review.merge_staged = false;
                review.merge_layout = PaneLayout::default();
                return;
            }
        };
        self.cjk_seen |= crate::system_fonts::contains_cjk(&snapshot.text);
        if purpose == Purpose::VerifyMerge {
            if snapshot.revision != content_revision(&snapshot.text) {
                review.snapshot = None;
                review.disk_layout = PaneLayout::default();
                self.clear_merge_preview(Some(
                    "Disk contents do not match their SHA-256 revision. Refresh and preview again"
                        .into(),
                ));
            } else if review
                .snapshot
                .as_ref()
                .is_some_and(|old| old.text == snapshot.text && old.revision == snapshot.revision)
            {
                review.merge_staged = true;
            } else {
                review.snapshot = Some(snapshot);
                review.disk_layout = PaneLayout::default();
                self.clear_merge_preview(Some("Disk changed again. Review the new snapshot and explicitly Preview merge again".into()));
            }
            return;
        }
        if purpose == Purpose::VerifyReload
            && review
                .snapshot
                .as_ref()
                .is_some_and(|old| old.text == snapshot.text && old.revision == snapshot.revision)
        {
            // Keep one copy: the verification body can be dropped immediately.
            // All editor/form/popup handlers run before finish_disk_reload.
            review.staged = true;
        } else {
            review.snapshot = Some(snapshot);
            review.disk_layout = PaneLayout::default();
            review.message = (purpose == Purpose::VerifyReload).then(|| {
                "Disk changed again. Review the updated snapshot, then click Reload clean tab again".into()
            });
        }
    }

    pub(super) fn finish_disk_reload(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        if !review.staged {
            return;
        }
        let problem = self.reload_source_problem(&review.source, true);
        if let Some(problem) = problem {
            let review = self.disk_review.slot.as_mut().unwrap();
            review.staged = false;
            review.message = Some(problem.into());
            return;
        }
        let mut review = self.disk_review.slot.take().unwrap();
        review.staged = false;
        let snapshot = review.snapshot.as_ref().unwrap();
        let doc = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == review.source.document)
            .unwrap();
        let text_changed = doc.text != snapshot.text;
        let baseline_changed = doc.revision.as_ref() != Some(&snapshot.revision);
        if text_changed {
            let cursor = editor_state::load(ctx, doc)
                .cursor
                .char_range()
                .map_or(0, |range| range.primary.index)
                .min(snapshot.text.chars().count());
            editor_state::commit(ctx, doc, snapshot.text.clone(), cursor);
            doc.jump_to = None;
            self.find_index = None;
        }
        if text_changed || baseline_changed {
            doc.adopt_reviewed_disk_baseline(snapshot.revision.clone());
            if doc.path == crate::profile_ui::PATH {
                self.profile_disk_reloaded();
            }
            review.message = Some("Reloaded the verified disk snapshot. Undo restores the previous text as an unsaved draft".into());
        } else {
            review.message =
                Some("The tab already matches the reviewed disk text and revision".into());
        }
        review.source = Source::capture(self).unwrap();
        self.disk_review.slot = Some(review);
    }

    pub(super) fn finish_disk_merge(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        if !review.merge_staged {
            return;
        }
        if let Some(problem) = self.merge_preview_problem(review, ctx) {
            self.clear_merge_preview(Some(problem.into()));
            return;
        }
        let mut review = self.disk_review.slot.take().unwrap();
        let preview = review.merge.take().unwrap();
        let snapshot = review.snapshot.as_ref().unwrap();
        let doc = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == review.source.document)
            .unwrap();
        let cursor = preview
            .selection
            .map_or(0, |range| range.primary.index)
            .min(preview.text.chars().count());
        editor_state::commit(ctx, doc, preview.text, cursor);
        // A merge's saved baseline is the separately reviewed disk text, never
        // the merged draft and never a write acknowledgement. Normal recovery
        // observation sees the dirty new tuple; existing ownership stays intact.
        doc.saved_text = snapshot.text.clone();
        doc.revision = Some(snapshot.revision.clone());
        doc.jump_to = None;
        self.find_index = None;
        self.replace.invalidate();
        if doc.path == crate::profile_ui::PATH {
            self.profile_disk_merged();
        }
        review.merge_staged = false;
        review.merge_layout = PaneLayout::default();
        review.source = Source::capture(self).unwrap();
        review.message = Some("Merged into the unsaved draft. Disk was not written. Undo restores your prior draft and selection; Save separately when ready".into());
        self.disk_review.slot = Some(review);
    }

    pub(super) fn disk_review_window(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.disk_review.slot else {
            return;
        };
        if !review.source.same_document(self) {
            self.dismiss_disk_review();
            return;
        }
        let busy = self.disk_review.busy();
        let reload_problem = self.reload_source_problem(&review.source, false);
        let can_reload = !busy && review.snapshot.is_some() && reload_problem.is_none();
        let has_merge = review.merge.is_some();
        let can_preview = !busy && review.snapshot.is_some();
        let mut visible = true;
        let mut refresh = false;
        let mut reload = false;
        let mut preview_merge = false;
        let mut apply_merge = false;
        let mut cancel_merge = false;
        let mut close = false;
        let review = self.disk_review.slot.as_mut().unwrap();
        let doc = self
            .documents
            .iter()
            .find(|doc| doc.id == review.source.document)
            .unwrap();
        egui::Window::new("Compare with disk")
            .id(egui::Id::new("disk_review"))
            .open(&mut visible).collapsible(false)
            .default_size([920.0, 520.0])
            .max_size(ctx.screen_rect().size() - egui::vec2(40.0, 80.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(&doc.path).color(GREEN));
                ui.label(RichText::new("Disk is read only on request. Reload needs a clean tab. Merge previews separate changes into the draft; Apply rechecks disk. Save separately.").small().color(MUTED));
                ui.horizontal_wrapped(|ui| {
                    refresh = ui.add_enabled(!busy, egui::Button::new("Refresh")).clicked();
                    reload = ui.add_enabled(can_reload, egui::Button::new("Reload clean tab")).clicked();
                    let preview = ui.add_enabled(can_preview, egui::Button::new("Preview merge"));
                    preview_merge = preview.clicked();
                    #[cfg(test)]
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("disk_merge_preview_button"), preview.rect));
                    if has_merge {
                        let apply = ui.add_enabled(!busy, egui::Button::new("Apply"));
                        let cancel = ui.button("Cancel merge");
                        apply_merge = apply.clicked();
                        cancel_merge = cancel.clicked();
                        #[cfg(test)]
                        ctx.data_mut(|data| {
                            data.insert_temp(egui::Id::new("disk_merge_apply_button"), apply.rect);
                            data.insert_temp(egui::Id::new("disk_merge_cancel_button"), cancel.rect);
                        });
                    }
                    close = ui.button("Close").clicked();
                    if busy { ui.spinner(); ui.label("Reading disk…"); }
                });
                if let Some(message) = &review.message { ui.label(message); }
                if !doc.dirty() {
                    if let Some(problem) = reload_problem { ui.label(RichText::new(problem).small().color(MUTED)); }
                }
                ui.separator();
                let height = ui.available_height().max(100.0);
                ui.columns(if has_merge { 3 } else { 2 }, |columns| {
                    columns[0].label("Your draft");
                    if columns[0].button("Copy draft").clicked() { ctx.copy_text(doc.text.clone()); }
                    egui::ScrollArea::both().id_salt("disk_review_draft").max_height(height).show(&mut columns[0], |ui| {
                        if doc.text.len() > MAX_FILE_BYTES {
                            review.draft_layout = PaneLayout::default();
                            ui.label("Draft exceeds 1 MiB. It remains intact in the editor; use Copy draft to keep the full text.");
                        } else {
                            review.draft_layout.show(ui, &doc.text, doc.edit_version);
                        }
                    });
                    columns[1].label("Disk at last read");
                    if let Some(snapshot) = &review.snapshot {
                        if columns[1].button("Copy disk version").clicked() { ctx.copy_text(snapshot.text.clone()); }
                        egui::ScrollArea::both().id_salt("disk_review_disk").max_height(height).show(&mut columns[1], |ui| {
                            review.disk_layout.show(ui, &snapshot.text, 0);
                        });
                    } else {
                        columns[1].label("No valid disk snapshot yet");
                    }
                    if let Some(preview) = &review.merge {
                        columns[2].label("Merge preview · unsaved");
                        if columns[2].button("Copy merge preview").clicked() { ctx.copy_text(preview.text.clone()); }
                        egui::ScrollArea::both().id_salt("disk_review_merge").max_height(height).show(&mut columns[2], |ui| {
                            review.merge_layout.show(ui, &preview.text, 0);
                        });
                    }
                });
            });
        if close
            || !visible
            || ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.dismiss_disk_review();
        } else if refresh {
            self.compare_with_disk();
        } else if reload {
            self.reload_from_disk();
        } else if cancel_merge {
            self.cancel_disk_merge();
        } else if preview_merge {
            self.preview_disk_merge(ctx);
        } else if apply_merge {
            self.apply_disk_merge(ctx);
        }
    }
}

#[cfg(test)]
mod tests;
