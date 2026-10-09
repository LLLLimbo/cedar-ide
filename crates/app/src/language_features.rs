//! Explicit, snapshot-checked edit previews and navigation. Server text stays inert.
use super::*;
use crate::{
    language_navigation_results::{self, OutlineItem, OutlineLocation},
    text_edits::{self, TextEdit},
};

#[derive(Clone, Debug)]
struct DocumentStamp {
    id: u64,
    path: String,
    edit_version: u64,
    text: String,
    lsp_version: Option<i32>,
}
impl DocumentStamp {
    fn capture(doc: &Document) -> Self {
        Self {
            id: doc.id,
            path: doc.path.clone(),
            edit_version: doc.edit_version,
            text: doc.text.clone(),
            lsp_version: None,
        }
    }
    fn matches(&self, documents: &[Document]) -> bool {
        documents.iter().any(|doc| {
            doc.id == self.id
                && doc.path == self.path
                && doc.edit_version == self.edit_version
                && doc.text == self.text
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeatureKind {
    Format { tab_size: u32, insert_spaces: bool },
    OrganizeJavaImports,
    JavaImplementations,
    References { include_declaration: bool },
    Outline,
}
#[derive(Clone, Debug)]
pub(crate) struct FeatureRequest {
    generation: u64,
    session: u64,
    pub(super) sequence: u64,
    pub(super) navigation: u64,
    source: DocumentStamp,
    participants: Vec<DocumentStamp>,
    cursor: Position,
    cursor_chars: usize,
    pub(super) kind: FeatureKind,
}
struct EditPreview {
    request: FeatureRequest,
    edits: Vec<TextEdit>,
    after: String,
    edit_count: usize,
}
struct OutlineSnapshot {
    request: FeatureRequest,
    items: Vec<OutlineItem>,
}
pub(super) struct FeatureState {
    sequence: u64,
    intent: Option<FeatureRequest>,
    preview: Option<EditPreview>,
    preview_open: bool,
    references: Vec<Location>,
    references_requested: bool,
    references_include_declaration: bool,
    outline: Option<OutlineSnapshot>,
    tab_size: u32,
    insert_spaces: bool,
    include_declaration: bool,
}
impl Default for FeatureState {
    fn default() -> Self {
        Self {
            sequence: 0,
            intent: None,
            preview: None,
            preview_open: false,
            references: vec![],
            references_requested: false,
            references_include_declaration: false,
            outline: None,
            tab_size: 4,
            insert_spaces: true,
            include_declaration: true,
        }
    }
}
impl FeatureState {
    pub(super) fn sequence(&self) -> u64 {
        self.sequence
    }
    pub(super) fn has_request_or_preview(&self) -> bool {
        self.intent.is_some() || self.preview.is_some()
    }
    pub(super) fn cancel_pending(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        self.intent = None;
        self.preview = None;
        self.preview_open = false;
    }
    pub(super) fn reset(&mut self) {
        self.cancel_pending();
        self.references.clear();
        self.references_requested = false;
        self.outline = None;
    }
}
enum FeatureStep {
    Sync(u64),
    Dispatch(Box<FeatureRequest>, Operation),
}
impl CedarApp {
    fn java_imports_problem(&self) -> Option<String> {
        if !self.ready() || !self.language.running || self.language.diagnostics_exited {
            return Some("Start a Java / JDT LS session for this connection first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Organize imports requires trusted tool permission for this connection".into(),
            );
        }
        if self.language.mode != ServerMode::Java {
            return Some("Organize imports requires a typed Java / JDT LS session".into());
        }
        if !self.backend_supports("language_organize_java_imports") {
            return Some(self.unsupported_message("language_organize_java_imports"));
        }
        if !self.language.java_organize_imports_supported {
            return Some("Organize imports is unavailable for this JDT LS session".into());
        }
        if !self.active().is_some_and(|doc| doc.path.ends_with(".java")) {
            return Some("Select a Java document to organize its imports".into());
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        None
    }
    pub(crate) fn java_imports_operation_problem(
        &self,
        path: &str,
        version: i32,
    ) -> Option<String> {
        if let Some(problem) = self.java_imports_problem() {
            return Some(problem);
        }
        let doc = self
            .active()
            .expect("import guard checked the active document");
        if path != doc.path
            || version <= 0
            || !self
                .language
                .sync
                .opened
                .get(&doc.id)
                .is_some_and(|ack| ack.version == version && ack.edit_version == doc.edit_version)
        {
            return Some(
                "Organize imports requires the active document's exact synchronized version".into(),
            );
        }
        None
    }
    // Cursor location is deliberately absent: moving it is not a document change.
    fn feature_document_current(&self, request: &FeatureRequest) -> bool {
        self.ready()
            && self.language.running
            && self.generation == request.generation
            && self.language.session == request.session
            && self.active_document == Some(request.source.id)
            && request.source.matches(&self.documents)
            && (!matches!(request.kind, FeatureKind::OrganizeJavaImports)
                || self.java_imports_problem().is_none())
            && (!matches!(request.kind, FeatureKind::JavaImplementations)
                || self.java_implementations_problem().is_none())
    }
    pub(super) fn feature_request_current(&self, request: &FeatureRequest) -> bool {
        self.feature_document_current(request)
            && self.language.features.sequence == request.sequence
            && (!matches!(request.kind, FeatureKind::JavaImplementations)
                || (request.navigation == self.navigation_epoch
                    && self.tools_open
                    && self.tool == crate::Tool::Language
                    && self.language.view == View::JavaImplementations))
            && request
                .participants
                .iter()
                .all(|stamp| stamp.matches(&self.documents))
            && (!matches!(
                request.kind,
                FeatureKind::References { .. } | FeatureKind::JavaImplementations
            ) || self.active().is_some_and(|doc| {
                utf16_position(&doc.text, doc.cursor).ok() == Some(request.cursor)
            }))
            && (!matches!(
                request.kind,
                FeatureKind::References { .. } | FeatureKind::JavaImplementations
            ) || self
                .documents
                .iter()
                .filter(|doc| self.language.matches(&doc.path))
                .count()
                == request.participants.len())
            && request.source.lsp_version.is_none_or(|version| {
                self.language
                    .sync
                    .opened
                    .get(&request.source.id)
                    .is_some_and(|ack| {
                        ack.version == version && ack.edit_version == request.source.edit_version
                    })
            })
            && request.participants.iter().all(|stamp| {
                stamp.lsp_version.is_none_or(|version| {
                    self.language.sync.opened.get(&stamp.id).is_some_and(|ack| {
                        ack.version == version && ack.edit_version == stamp.edit_version
                    })
                })
            })
    }
    pub(super) fn request_language_navigation_feature(&mut self, kind: FeatureKind) {
        if !self.ready() || !self.language.running {
            self.error = Some("Start a language server first".into());
            return;
        }
        let remote_capability = match kind {
            FeatureKind::Format { .. } => "language_format",
            FeatureKind::OrganizeJavaImports => "language_organize_java_imports",
            FeatureKind::JavaImplementations => "language_java_implementations",
            FeatureKind::References { .. } => "language_references",
            FeatureKind::Outline => "language_document_symbols",
        };
        if !self.backend_supports(remote_capability) {
            self.error = Some(self.unsupported_message(remote_capability));
            return;
        }
        let capability = match kind {
            FeatureKind::Format { .. } => Some("documentFormattingProvider"),
            FeatureKind::OrganizeJavaImports => {
                if let Some(problem) = self.java_imports_problem() {
                    self.error = Some(problem);
                    return;
                }
                None
            }
            FeatureKind::JavaImplementations => {
                if let Some(problem) = self.java_implementations_problem() {
                    self.error = Some(problem);
                    return;
                }
                Some("implementationProvider")
            }
            FeatureKind::References { .. } => Some("referencesProvider"),
            FeatureKind::Outline => Some("documentSymbolProvider"),
        };
        if capability.is_some_and(|capability| !self.language.supports(capability)) {
            self.error = Some("The running language server does not advertise this feature".into());
            return;
        }
        let Some(doc) = self.active() else {
            return;
        };
        if !self.language.matches(&doc.path) {
            self.error =
                Some("This file does not match the running server’s language profile".into());
            return;
        }
        let cursor = match utf16_position(&doc.text, doc.cursor) {
            Ok(position) => position,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let cursor_chars = match completion::position_to_offsets(&doc.text, cursor) {
            Ok((_, offset)) => offset,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let source = DocumentStamp::capture(doc);
        let participants: Vec<_> = if matches!(
            kind,
            FeatureKind::References { .. } | FeatureKind::JavaImplementations
        ) {
            self.documents
                .iter()
                .filter(|doc| self.language.matches(&doc.path))
                .map(DocumentStamp::capture)
                .collect()
        } else {
            vec![]
        };
        if source.text.len() > MAX_FILE_BYTES
            || participants
                .iter()
                .any(|stamp| stamp.text.len() > MAX_FILE_BYTES)
        {
            self.error = Some(
                "Each participating draft must fit the 1 MiB language limit; nothing was changed"
                    .into(),
            );
            return;
        }
        self.language.features.cancel_pending();
        self.language.intent = None;
        self.language.completions = None;
        self.language.completion_popup = false;
        self.language.acceptance_sequence = self.language.acceptance_sequence.wrapping_add(1);
        self.language.features.intent = Some(FeatureRequest {
            generation: self.generation,
            session: self.language.session,
            sequence: self.language.features.sequence,
            navigation: self.navigation_epoch,
            source,
            participants,
            cursor,
            cursor_chars,
            kind,
        });
        if let Some(request) = &self.language.features.intent {
            self.language.sync.retry(request.source.id);
            for stamp in &request.participants {
                self.language.sync.retry(stamp.id);
            }
        }
        self.language.paused_reason = None;
        self.tools_open = true;
        self.tool = crate::Tool::Language;
        self.language.view = match kind {
            FeatureKind::Format { .. } => View::Format,
            FeatureKind::OrganizeJavaImports => View::Imports,
            FeatureKind::JavaImplementations => View::JavaImplementations,
            FeatureKind::References { .. } => View::References,
            FeatureKind::Outline => View::Outline,
        };
        if matches!(kind, FeatureKind::JavaImplementations) {
            self.language.implementations.begin(
                self.language
                    .features
                    .intent
                    .clone()
                    .expect("captured request"),
            );
        }
        self.notice =
            "Synchronizing the exact draft snapshot before requesting language results".into();
    }
    pub(super) fn invalidate_language_features(&mut self) {
        let invalid_intent = self
            .language
            .features
            .intent
            .as_ref()
            .is_some_and(|request| !self.feature_request_current(request));
        let invalid_preview = self
            .language
            .features
            .preview
            .as_ref()
            .is_some_and(|preview| !self.feature_request_current(&preview.request));
        if invalid_intent || invalid_preview {
            self.language.features.cancel_pending();
            self.notice = "Language snapshot expired; request it again".into();
        }
        if self
            .language
            .features
            .outline
            .as_ref()
            .is_some_and(|outline| !self.feature_document_current(&outline.request))
        {
            self.language.features.outline = None;
        }
    }
    fn next_language_feature_step(&mut self) -> Option<FeatureStep> {
        self.invalidate_language_features();
        let mut request = self.language.features.intent.clone()?;
        for stamp in std::iter::once(&request.source).chain(&request.participants) {
            if !self.language.sync.synced(stamp.id, stamp.edit_version) {
                return Some(FeatureStep::Sync(stamp.id));
            }
        }
        request.source.lsp_version = self
            .language
            .sync
            .opened
            .get(&request.source.id)
            .map(|ack| ack.version);
        for stamp in &mut request.participants {
            stamp.lsp_version = self
                .language
                .sync
                .opened
                .get(&stamp.id)
                .map(|ack| ack.version);
        }
        // Capture the current cursor only after the exact document sync. Later cursor-only
        // movement is allowed; Apply maps that latest cursor through the same checked edits.
        if let Some(doc) = self.active() {
            if !matches!(
                request.kind,
                FeatureKind::References { .. } | FeatureKind::JavaImplementations
            ) {
                request.cursor = utf16_position(&doc.text, doc.cursor).ok()?;
                request.cursor_chars = completion::position_to_offsets(&doc.text, request.cursor)
                    .ok()?
                    .1;
            }
        }
        let op = match request.kind {
            FeatureKind::Format {
                tab_size,
                insert_spaces,
            } => Operation::LanguageFormat {
                path: request.source.path.clone(),
                version: request.source.lsp_version?,
                tab_size,
                insert_spaces,
            },
            FeatureKind::OrganizeJavaImports => Operation::LanguageOrganizeJavaImports {
                path: request.source.path.clone(),
                version: request.source.lsp_version?,
            },
            FeatureKind::JavaImplementations => Operation::LanguageJavaImplementations {
                path: request.source.path.clone(),
                version: request.source.lsp_version?,
                line: request.cursor.line,
                character: request.cursor.character,
            },
            FeatureKind::References {
                include_declaration,
            } => Operation::LanguageReferences {
                path: request.source.path.clone(),
                line: request.cursor.line,
                character: request.cursor.character,
                include_declaration,
            },
            FeatureKind::Outline => Operation::LanguageDocumentSymbols {
                path: request.source.path.clone(),
            },
        };
        self.language.features.intent = None;
        if matches!(request.kind, FeatureKind::JavaImplementations) {
            self.language.implementations.begin(request.clone());
        }
        Some(FeatureStep::Dispatch(Box::new(request), op))
    }
    pub(super) fn language_feature_tick(&mut self) -> bool {
        match self.next_language_feature_step() {
            Some(FeatureStep::Sync(id)) => {
                self.sync_document(id);
                true
            }
            Some(FeatureStep::Dispatch(request, op)) => {
                self.language_request(op, ActionKind::Feature { request: *request });
                true
            }
            None => false,
        }
    }
    pub(super) fn apply_language_feature(&mut self, request: FeatureRequest, value: Value) {
        if !self.feature_request_current(&request) {
            self.notice = "Stale language result ignored; request a fresh snapshot".into();
            return;
        }
        match request.kind {
            FeatureKind::Format { .. } | FeatureKind::OrganizeJavaImports => {
                let organizing = matches!(request.kind, FeatureKind::OrganizeJavaImports);
                if organizing && !value.is_array() {
                    self.error = Some("Import edits must be a plain text edit array".into());
                    return;
                }
                let edits = match text_edits::parse_text_edits(&value) {
                    Ok(edits) => edits,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                let plan = match text_edits::plan_text_edits(
                    &request.source.text,
                    &edits,
                    request.cursor_chars,
                ) {
                    Ok(plan) => plan,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                if plan.text == request.source.text {
                    self.language.features.cancel_pending();
                    self.notice = if organizing {
                        if edits.is_empty() {
                            "No import edits returned; unresolved types may remain"
                        } else {
                            "Organize imports made no text changes; unresolved types may remain"
                        }
                    } else {
                        "Formatting made no changes"
                    }
                    .into();
                    return;
                }
                self.language.cjk_seen |= crate::system_fonts::contains_cjk(&plan.text);
                self.language.features.preview = Some(EditPreview {
                    request,
                    edits,
                    after: plan.text,
                    edit_count: plan.edit_count,
                });
                self.language.features.preview_open = true;
                self.language.view = if organizing {
                    View::Imports
                } else {
                    View::Format
                };
                self.notice = if organizing {
                    "Organize imports preview ready. Apply changes only the unsaved draft"
                } else {
                    "Formatting preview ready. Apply changes only the unsaved draft"
                }
                .into();
            }
            FeatureKind::References {
                include_declaration,
            } => match language_navigation_results::parse_references(&value) {
                Ok(locations) => {
                    self.language.features.references = locations;
                    self.language.features.references_requested = true;
                    self.language.features.references_include_declaration = include_declaration;
                    self.language.view = View::References;
                }
                Err(error) => self.error = Some(error),
            },
            FeatureKind::JavaImplementations => self.apply_java_implementations(request, value),
            FeatureKind::Outline => {
                match language_navigation_results::parse_outline(&value, &request.source.text) {
                    Ok(items) => {
                        self.language.features.outline = Some(OutlineSnapshot { request, items });
                        self.language.view = View::Outline;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
    }
    fn apply_format_preview(&mut self) {
        let Some(preview) = self.language.features.preview.take() else {
            return;
        };
        let label = if matches!(preview.request.kind, FeatureKind::OrganizeJavaImports) {
            "Organize imports"
        } else {
            "Formatting"
        };
        self.language.features.preview_open = false;
        if !self.feature_request_current(&preview.request) {
            self.language.features.cancel_pending();
            self.error = Some(format!(
                "{label} preview expired. Nothing was applied; request it again"
            ));
            return;
        }
        let cursor_chars = self.active().and_then(|doc| {
            utf16_position(&doc.text, doc.cursor)
                .ok()
                .and_then(|cursor| {
                    completion::position_to_offsets(&doc.text, cursor)
                        .ok()
                        .map(|(_, index)| index)
                })
        });
        let Some(cursor_chars) = cursor_chars else {
            self.error =
                Some("The current cursor cannot be mapped safely; nothing was applied".into());
            return;
        };
        let plan = match text_edits::plan_text_edits(
            &preview.request.source.text,
            &preview.edits,
            cursor_chars,
        ) {
            Ok(plan) if plan.text == preview.after => plan,
            _ => {
                self.error = Some(format!(
                    "{label} preview could not be revalidated; nothing was applied"
                ));
                return;
            }
        };
        self.language.features.cancel_pending();
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == preview.request.source.id)
        {
            // Do not authorize recovery: normal observation must protect unrelated old copies.
            crate::editor_state::commit(&self.editor_ctx, doc, plan.text, plan.cursor_chars);
            self.notice = format!(
                "Applied {} {} edits to the unsaved draft; one undo step",
                plan.edit_count,
                if matches!(preview.request.kind, FeatureKind::OrganizeJavaImports) {
                    "import"
                } else {
                    "formatting"
                }
            );
        }
        self.language.features.outline = None;
    }
    pub(super) fn language_feature_controls(&mut self, ui: &mut egui::Ui) {
        if self.language.mode == ServerMode::Java {
            ui.horizontal_wrapped(|ui| self.java_implementations_control(ui));
        }
        if !self.language.running {
            return;
        }
        let matching = self.ready()
            && self
                .active()
                .is_some_and(|doc| self.language.matches(&doc.path));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.java_imports_problem().is_none(), egui::Button::new("Organize imports")).on_hover_text(IMPORTS_DISCLOSURE).clicked() {
                self.request_language_navigation_feature(FeatureKind::OrganizeJavaImports);
            }
            if ui.add_enabled(matching && self.backend_supports("language_format") && self.language.supports("documentFormattingProvider"), egui::Button::new("Format preview")).clicked() {
                self.request_language_navigation_feature(FeatureKind::Format { tab_size: self.language.features.tab_size, insert_spaces: self.language.features.insert_spaces });
            }
            ui.label("Indent");
            ui.add(egui::DragValue::new(&mut self.language.features.tab_size).range(1..=16));
            ui.checkbox(&mut self.language.features.insert_spaces, "Spaces");
            if ui.add_enabled(matching && self.backend_supports("language_references") && self.language.supports("referencesProvider"), egui::Button::new("Find references")).clicked() {
                self.request_language_navigation_feature(FeatureKind::References { include_declaration: self.language.features.include_declaration });
            }
            ui.checkbox(&mut self.language.features.include_declaration, "Include declaration");
            if ui.add_enabled(matching && self.backend_supports("language_document_symbols") && self.language.supports("documentSymbolProvider"), egui::Button::new("Refresh outline")).clicked() {
                self.request_language_navigation_feature(FeatureKind::Outline);
            }
            let pending = self.language.features.intent.is_some() || self.pending.values().any(|job| matches!(job, Job::Language(Action { kind: ActionKind::Feature { request }, .. }) if request.sequence == self.language.features.sequence));
            if pending && ui.button("Cancel request").clicked() {
                self.language.features.cancel_pending();
                self.notice = "Language request cancelled; any late result will be ignored".into();
            }
        });
    }
    pub(super) fn edit_preview_view(&mut self, ui: &mut egui::Ui) {
        let organizing = self.language.view == View::Imports;
        if organizing {
            ui.label(RichText::new(IMPORTS_DISCLOSURE).small().color(AMBER));
        }
        if let Some(preview) = self.language.features.preview.as_ref().filter(|preview| {
            matches!(preview.request.kind, FeatureKind::OrganizeJavaImports) == organizing
        }) {
            ui.label(format!(
                "{} · {} edits · unsaved draft only",
                preview.request.source.path, preview.edit_count
            ));
            if ui
                .button(if organizing {
                    "Review import preview"
                } else {
                    "Review formatting preview"
                })
                .clicked()
            {
                self.language.features.preview_open = true;
            }
        } else {
            ui.label(if organizing {
                "Choose Organize imports to review the complete before/after text before applying"
            } else {
                "Request Format preview to review the complete before/after text before applying"
            });
        }
    }
    pub(super) fn references_view(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Unversioned server snapshot. Open drafts were synchronized before the request; any target may have changed since. Ranges are checked when opened.").small().color(AMBER));
        if self.language.features.references_requested {
            ui.label(format!(
                "{} references · declaration {}",
                self.language.features.references.len(),
                if self.language.features.references_include_declaration {
                    "included"
                } else {
                    "excluded"
                }
            ));
        }
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("references")
            .show(ui, |ui| {
                for location in &self.language.features.references {
                    if ui
                        .add_enabled(
                            self.backend_supports("language_resolve_uri"),
                            egui::Button::new(format!(
                                "{}:{}:{}",
                                location.uri,
                                u64::from(location.range.start.line) + 1,
                                u64::from(location.range.start.character) + 1
                            )),
                        )
                        .clicked()
                    {
                        selected = Some(location.clone());
                    }
                }
                if self.language.features.references.is_empty() {
                    ui.label(if self.language.features.references_requested {
                        "No references returned"
                    } else {
                        "Place the cursor on a symbol, then choose Find references"
                    });
                }
            });
        if let Some(location) = selected {
            self.navigate_language(location);
        }
    }
    pub(super) fn outline_view(&mut self, ui: &mut egui::Ui) {
        self.invalidate_language_features();
        let mut selected = None;
        if let Some(outline) = &self.language.features.outline {
            ui.label(
                RichText::new(format!(
                    "{} · captured draft outline · explicit refresh",
                    outline.request.source.path
                ))
                .small()
                .color(MUTED),
            );
            if outline
                .items
                .iter()
                .any(|item| matches!(item.location, OutlineLocation::Remote(_)))
            {
                ui.label(
                    RichText::new("Flat symbol targets are unversioned and may have changed")
                        .small()
                        .color(AMBER),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt("document_outline")
                .show(ui, |ui| {
                    for item in &outline.items {
                        ui.horizontal(|ui| {
                            ui.add_space((item.depth.min(12) * 12) as f32);
                            ui.label(RichText::new(symbol_kind(item.kind)).small().color(MUTED));
                            let navigable = !matches!(item.location, OutlineLocation::Remote(_))
                                || self.backend_supports("language_resolve_uri");
                            if ui
                                .add_enabled(navigable, egui::Button::new(&item.name))
                                .on_hover_text(&item.detail)
                                .clicked()
                            {
                                selected = Some(item.location.clone());
                            }
                            if !item.detail.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&item.detail).small().color(MUTED),
                                    )
                                    .truncate(),
                                );
                            }
                        });
                    }
                    if outline.items.is_empty() {
                        ui.label("No document symbols returned");
                    }
                });
        } else {
            ui.label(
                "Choose Refresh outline for the current draft. Editing invalidates this snapshot",
            );
        }
        if let Some(location) = selected {
            self.navigate_outline_location(location);
        }
    }
    fn navigate_outline_location(&mut self, location: OutlineLocation) {
        match location {
            OutlineLocation::Local { range, selection } => {
                if selection.start < range.start || selection.end > range.end {
                    return;
                }
                let Some(outline) = &self.language.features.outline else {
                    return;
                };
                if !self.feature_document_current(&outline.request) {
                    return;
                }
                let document = outline.request.source.id;
                let Some(doc) = self.documents.iter().find(|doc| doc.id == document) else {
                    return;
                };
                let (Ok((_, start)), Ok((_, end))) = (
                    completion::position_to_offsets(&doc.text, selection.start),
                    completion::position_to_offsets(&doc.text, selection.end),
                ) else {
                    return;
                };
                // A local symbol click is newer navigation. Invalidate both URI resolution
                // and already-dispatched file opens, without cancelling document-only work
                // for what is otherwise a cursor movement in the same unchanged draft.
                let departure = self.history_departure();
                self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
                self.language.cancel_deferred_navigation();
                self.history_begin(departure, false);
                let ticket = self.history_take_completion();
                let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == document) else {
                    return;
                };
                let id = egui::Id::new(("editor", doc.id));
                let mut state = crate::editor_state::load(&self.editor_ctx, doc);
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(start),
                        egui::text::CCursor::new(end),
                    )));
                state.store(&self.editor_ctx, id);
                doc.cursor = crate::model::cursor_location(&doc.text, end);
                doc.scroll_to = Some(start);
                self.editor_ctx
                    .memory_mut(|memory| memory.request_focus(id));
                self.history_commit(ticket);
            }
            OutlineLocation::Remote(location) => self.navigate_language(location),
        }
    }
    fn cancel_format_preview(&mut self) {
        let organizing = self
            .language
            .features
            .preview
            .as_ref()
            .is_some_and(|preview| {
                matches!(preview.request.kind, FeatureKind::OrganizeJavaImports)
            });
        self.language.features.cancel_pending();
        self.notice = if organizing {
            "Organize imports cancelled; draft unchanged"
        } else {
            "Formatting cancelled; draft unchanged"
        }
        .into();
    }
    pub(super) fn format_preview_shortcut(&mut self, ctx: &egui::Context) -> bool {
        if self.language.features.preview_open
            && self.language.features.preview.is_some()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.cancel_format_preview();
            true
        } else {
            false
        }
    }
    pub(super) fn format_preview_window(&mut self, ctx: &egui::Context) {
        self.invalidate_language_features();
        if !self.language.features.preview_open {
            return;
        }
        let Some(preview) = self.language.features.preview.as_mut() else {
            return;
        };
        let mut visible = true;
        let mut apply = false;
        let mut cancel = false;
        let organizing = matches!(preview.request.kind, FeatureKind::OrganizeJavaImports);
        egui::Window::new(if organizing { "Organize imports preview" } else { "Formatting preview" })
            .id(egui::Id::new("format_preview"))
            .open(&mut visible).collapsible(false).resizable(true)
            .default_size(egui::vec2(920.0, 520.0))
            .max_size(ctx.screen_rect().size() - egui::vec2(40.0, 80.0))
            .show(ctx, |ui| {
                ui.label(format!("{} · {} edits", preview.request.source.path, preview.edit_count));
                ui.label(RichText::new("Read-only preview. Apply changes the draft in one undo step; Save remains separate.").small().color(MUTED));
                if organizing {
                    ui.label(RichText::new(IMPORTS_DISCLOSURE).small().color(AMBER));
                }
                ui.horizontal(|ui| {
                    apply = ui.button("Apply to draft").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
                ui.separator();
                let height = ui.available_height().max(100.0);
                ui.columns(2, |columns| {
                    columns[0].label("Before");
                    egui::ScrollArea::both().id_salt("format_before_scroll").max_height(height).show(&mut columns[0], |ui| {
                        ui.add(egui::TextEdit::multiline(&mut preview.request.source.text).id_salt("format_before_text").font(egui::TextStyle::Monospace).interactive(false).desired_width(f32::INFINITY));
                    });
                    columns[1].label("After");
                    egui::ScrollArea::both().id_salt("format_after_scroll").max_height(height).show(&mut columns[1], |ui| {
                        ui.add(egui::TextEdit::multiline(&mut preview.after).id_salt("format_after_text").font(egui::TextStyle::Monospace).interactive(false).desired_width(f32::INFINITY));
                    });
                });
            });
        if cancel
            || !visible
            || ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.cancel_format_preview();
        } else if apply {
            self.apply_format_preview();
        }
    }
}
const IMPORTS_DISCLOSURE: &str = "Sorts and removes imports, and adds uniquely resolved imports. Ambiguous missing types remain unresolved.";
fn symbol_kind(kind: u32) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type",
        _ => "symbol",
    }
}
#[cfg(test)]
#[path = "language_features_tests.rs"]
mod tests;
