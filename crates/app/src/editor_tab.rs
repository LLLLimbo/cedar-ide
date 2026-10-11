//! Tab edits require the exact editor that owned the original native batch.
//! This proof never transfers focus, removes events, or survives into a new batch.
use crate::{block_indent, model::Document, CedarApp, ConnectionState, WorkspaceKey};
use eframe::egui;

pub(super) struct FrameInput {
    pass: u64,
    viewport: egui::ViewportId,
    focused: Option<egui::Id>,
    window_focused: bool,
    eligible: bool,
    document: u64,
    path: String,
    version: u64,
    generation: u64,
    workspace: Option<WorkspaceKey>,
    root: String,
    connection: ConnectionState,
    session: u64,
    navigation: u64,
    pub block_action: bool,
}

fn pressed_tab(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key {
            key: egui::Key::Tab,
            pressed: true,
            ..
        }
    )
}

pub(super) fn has_tab(ctx: &egui::Context) -> bool {
    ctx.input(|input| input.events.iter().any(pressed_tab))
}

impl FrameInput {
    pub(super) fn capture(
        app: &CedarApp,
        ctx: &egui::Context,
        raw: &egui::RawInput,
    ) -> Option<Self> {
        let doc = app.active()?;
        if !raw.events.iter().any(pressed_tab) {
            return None;
        }
        Some(Self {
            pass: ctx.cumulative_pass_nr(),
            viewport: raw.viewport_id,
            focused: ctx.memory(|memory| memory.focused()),
            window_focused: raw.focused,
            eligible: app.editor_indent_eligible(ctx)
                && !app.navigation.coordinate_jump_pending()
                && doc.jump_to.is_none(),
            document: doc.id,
            path: doc.path.clone(),
            version: doc.edit_version,
            generation: app.generation,
            workspace: app.workspace_key.clone(),
            root: app.root.clone(),
            connection: app.state,
            session: app.language.session,
            navigation: app.navigation_epoch,
            block_action: block_indent::clean_tab_batch(&raw.events),
        })
    }

    pub(super) fn matches_context(&self, app: &CedarApp, ctx: &egui::Context) -> bool {
        let editor = egui::Id::new(("editor", self.document));
        self.focused == Some(editor)
            && self.matches_identity(app, ctx)
            && ctx.memory(|memory| memory.has_focus(editor))
    }

    pub(super) fn matches_identity(&self, app: &CedarApp, ctx: &egui::Context) -> bool {
        self.matches_frame(ctx)
            && self.matches_lifecycle(
                app.generation,
                &app.workspace_key,
                &app.root,
                app.state,
                app.language.session,
                app.navigation_epoch,
            )
            && app.active_document == Some(self.document)
    }

    pub(super) fn input_eligible(&self, app: &CedarApp, ctx: &egui::Context) -> bool {
        self.original_input_eligible()
            && ctx.input(|input| input.focused)
            && app.editor_indent_eligible(ctx)
            && !app.navigation.coordinate_jump_pending()
    }

    pub(super) fn original_input_eligible(&self) -> bool {
        self.window_focused && self.eligible
    }

    pub(super) fn matches_frame(&self, ctx: &egui::Context) -> bool {
        // egui counts completed render passes, not native input batches. The
        // original first-pass identity remains valid through a sizing retry.
        self.pass
            == ctx
                .cumulative_pass_nr()
                .saturating_sub(ctx.output(|output| output.num_completed_passes) as u64)
            && self.viewport == ctx.viewport_id()
    }

    pub(super) fn matches_lifecycle(
        &self,
        generation: u64,
        workspace: &Option<WorkspaceKey>,
        root: &str,
        connection: ConnectionState,
        session: u64,
        navigation: u64,
    ) -> bool {
        self.generation == generation
            && self.workspace.as_ref() == workspace.as_ref()
            && self.root == root
            && self.connection == connection
            && self.session == session
            && self.navigation == navigation
    }

    pub(super) fn matches_document_identity(&self, doc: &Document) -> bool {
        self.document == doc.id && self.path == doc.path
    }

    pub(super) fn matches_document(&self, doc: &Document) -> bool {
        self.matches_document_identity(doc) && self.version == doc.edit_version
    }
}
