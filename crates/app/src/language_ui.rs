//! Deliberate, manual LSP controls. Servers execute only after explicit user action.
use crate::{model::Document, CedarApp, Job, AMBER, GREEN, MUTED};
use cedar_protocol::{LanguageQueryKind, Operation};
use eframe::egui::{self, RichText};
use std::collections::HashMap;

pub(super) enum Action {
    Start,
    Stop,
    Sync {
        document: u64,
        version: i32,
        edit_version: u64,
    },
    Close {
        document: u64,
    },
    Query,
    Events,
}

pub(super) struct LanguagePanel {
    pub running: bool,
    pub opened: HashMap<u64, (i32, u64)>,
    program: String,
    args: String,
    language_id: String,
    output: String,
}
impl Default for LanguagePanel {
    fn default() -> Self {
        Self {
            running: false, opened: HashMap::new(), program: String::new(), args: "[]".into(),
            language_id: "rust".into(),
            output: "Start an installed stdio language server, sync a file, then request language information. Java/Kotlin servers and a JDK must be installed separately on the workspace host.".into(),
        }
    }
}
impl LanguagePanel {
    pub fn reset(&mut self) {
        self.running = false;
        self.opened.clear();
        self.output =
            "Language session stopped. Start a server for this connection when needed.".into();
    }
    pub fn apply(&mut self, action: Action, value: serde_json::Value) {
        match action {
            Action::Start => {
                self.running = true;
                self.opened.clear();
            }
            Action::Stop => {
                self.running = false;
                self.opened.clear();
            }
            Action::Sync {
                document,
                version,
                edit_version,
            } => {
                self.opened.insert(document, (version, edit_version));
            }
            Action::Close { document } => {
                self.opened.remove(&document);
            }
            Action::Query | Action::Events => {}
        }
        let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        if output.len() > 128 * 1024 {
            let mut end = 128 * 1024;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
            output.push_str("\n[Display truncated at 128 KiB]");
        }
        self.output = output;
    }
    fn synced(&self, doc: &Document) -> bool {
        self.opened
            .get(&doc.id)
            .is_some_and(|(_, version)| *version == doc.edit_version)
    }
}

impl CedarApp {
    pub(super) fn language_busy(&self) -> bool {
        self.pending
            .values()
            .any(|job| matches!(job, Job::Language(_)))
    }
    fn start_language(&mut self) {
        let args: Vec<String> = match serde_json::from_str(&self.language.args) {
            Ok(args) => args,
            Err(_) => {
                self.error =
                    Some("Language server arguments must be a JSON array of strings".into());
                return;
            }
        };
        let program = self.language.program.trim().to_owned();
        if program.is_empty() {
            self.error = Some("Enter the installed language server executable".into());
            return;
        }
        self.request(
            Operation::LanguageStart { program, args },
            Job::Language(Action::Start),
        );
    }
    fn sync_language(&mut self) {
        let Some(doc) = self.active() else {
            return;
        };
        let document = doc.id;
        let edit_version = doc.edit_version;
        let version = self
            .language
            .opened
            .get(&document)
            .map_or(1, |(version, _)| version.saturating_add(1));
        let op = if self.language.opened.contains_key(&document) {
            Operation::LanguageChange {
                path: doc.path.clone(),
                version,
                text: doc.text.clone(),
            }
        } else {
            Operation::LanguageOpen {
                path: doc.path.clone(),
                language_id: self.language.language_id.trim().to_owned(),
                version,
                text: doc.text.clone(),
            }
        };
        self.request(
            op,
            Job::Language(Action::Sync {
                document,
                version,
                edit_version,
            }),
        );
    }
    fn query_language(&mut self, kind: LanguageQueryKind) {
        let Some(doc) = self.active() else {
            return;
        };
        if !self.language.synced(doc) {
            self.error = Some("Sync this draft to the language server before querying it".into());
            return;
        }
        let (line, character) = utf16_position(&doc.text, doc.cursor);
        self.request(
            Operation::LanguageQuery {
                path: doc.path.clone(),
                line,
                character,
                kind,
            },
            Job::Language(Action::Query),
        );
    }
    pub(super) fn language_panel(&mut self, ui: &mut egui::Ui) {
        let trusted = self.active_form.as_ref().is_some_and(|form| form.allow_run);
        let busy = self.language_busy();
        if !trusted {
            ui.colored_label(AMBER, "Language servers require trusted command permission. Enable it in Open workspace and reconnect.");
        }
        ui.add_enabled_ui(trusted && self.ready() && !busy, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Server");
                ui.add_enabled(
                    !self.language.running,
                    egui::TextEdit::singleline(&mut self.language.program)
                        .hint_text("rust-analyzer / jdtls / server path")
                        .desired_width(260.0),
                );
                ui.label("Arguments (JSON)");
                ui.add_enabled(
                    !self.language.running,
                    egui::TextEdit::singleline(&mut self.language.args).desired_width(220.0),
                );
                if !self.language.running {
                    if ui.button("Start server").clicked() {
                        self.start_language();
                    }
                } else if ui.button("Stop server").clicked() {
                    self.request(Operation::LanguageStop, Job::Language(Action::Stop));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Language ID");
                ui.add(
                    egui::TextEdit::singleline(&mut self.language.language_id)
                        .hint_text("rust / java / kotlin")
                        .desired_width(85.0),
                );
                let document_ready = self.active().is_some_and(|doc| doc.revision.is_some());
                let synced = self.active().is_some_and(|doc| self.language.synced(doc));
                if ui
                    .add_enabled(
                        self.language.running
                            && document_ready
                            && !self.language.language_id.trim().is_empty(),
                        egui::Button::new("Sync current file"),
                    )
                    .clicked()
                {
                    self.sync_language();
                }
                let can_query = self.language.running && synced;
                if ui
                    .add_enabled(can_query, egui::Button::new("Hover"))
                    .clicked()
                {
                    self.query_language(LanguageQueryKind::Hover);
                }
                if ui
                    .add_enabled(can_query, egui::Button::new("Definition"))
                    .clicked()
                {
                    self.query_language(LanguageQueryKind::Definition);
                }
                if ui
                    .add_enabled(can_query, egui::Button::new("Completion"))
                    .clicked()
                {
                    self.query_language(LanguageQueryKind::Completion);
                }
                if ui
                    .add_enabled(
                        self.language.running,
                        egui::Button::new("Refresh diagnostics / events"),
                    )
                    .clicked()
                {
                    self.request(Operation::LanguageEvents, Job::Language(Action::Events));
                }
                if self.language.running {
                    ui.colored_label(
                        if synced { GREEN } else { AMBER },
                        if synced {
                            "Draft synced"
                        } else {
                            "Sync needed"
                        },
                    );
                }
            });
        });
        ui.label(RichText::new("Uses the editor cursor (UTF-16). Results are read-only JSON; changes and diagnostics are refreshed manually. Servers may index or execute project code.").small().color(MUTED));
        if busy {
            ui.spinner();
        }
        egui::ScrollArea::both()
            .id_salt("language_output")
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.language.output)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false)
                        .frame(false),
                );
            });
    }
}

fn utf16_position(text: &str, cursor: (usize, usize)) -> (u32, u32) {
    let line = cursor.0.saturating_sub(1);
    let column: usize = text
        .split('\n')
        .nth(line)
        .unwrap_or("")
        .chars()
        .take(cursor.1.saturating_sub(1))
        .map(char::len_utf16)
        .sum();
    (
        line.min(u32::MAX as usize) as u32,
        column.min(u32::MAX as usize) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_cursor_to_utf16() {
        assert_eq!(utf16_position("first\nA🐻é.end", (2, 4)), (1, 4));
    }
    #[test]
    fn typing_after_sync_requires_another_sync() {
        let mut panel = LanguagePanel::default();
        let mut doc = Document::new(1, "a.rs".into(), "fn main(){}".into(), "r".into());
        panel.apply(
            Action::Sync {
                document: 1,
                version: 1,
                edit_version: 0,
            },
            serde_json::json!({"synced":true}),
        );
        assert!(panel.synced(&doc));
        doc.edit_version += 1;
        assert!(!panel.synced(&doc));
    }
}
