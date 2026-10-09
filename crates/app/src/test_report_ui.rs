//! Explicit, bounded ordinary file reads. A report is a historical snapshot,
//! separate from editor buffers, command state, and language diagnostics.
use crate::{
    test_reports::{parse_report, TestCase, TestReport, TestStatus, MAX_REPORT_BYTES},
    CedarApp, Job, Operation, Payload, AMBER, GREEN, MUTED, RED,
};
use eframe::egui::{self, RichText};
use sha2::{Digest, Sha256};

const MAX_PATH_BYTES: usize = 4096;
const MAX_PENDING_READS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Load {
    pub generation: u64,
    pub id: u64,
    pub path: String,
}

pub(super) struct Snapshot {
    pub source: Load,
    pub revision: String,
    pub report: TestReport,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum Filter {
    #[default]
    All,
    Passed,
    Failed,
    Error,
    Skipped,
    Unsupported,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Error => "Errors",
            Self::Skipped => "Skipped",
            Self::Unsupported => "Unsupported / flaky",
        }
    }

    fn matches(self, status: TestStatus) -> bool {
        match self {
            Self::All => true,
            Self::Passed => status == TestStatus::Passed,
            Self::Failed => status == TestStatus::Failed,
            Self::Error => status == TestStatus::Error,
            Self::Skipped => status == TestStatus::Skipped,
            Self::Unsupported => status == TestStatus::Unsupported,
        }
    }
}

#[derive(Default)]
pub(super) struct TestReportPanel {
    pub path: String,
    next_load: u64,
    pub loading: Option<Load>,
    pub snapshot: Option<Snapshot>,
    pub message: Option<String>,
    pub filter: Filter,
    pub name_filter: String,
    pub selected: Option<usize>,
}

impl TestReportPanel {
    pub fn clear(&mut self) {
        self.loading = None;
        self.snapshot = None;
        self.message = None;
        self.selected = None;
        self.filter = Filter::All;
        self.name_filter.clear();
        // IDs are monotonic for the whole frontend lifetime, including clears
        // and reconnects. Cleared wire reads remain bounded in CedarApp.pending.
    }

    pub fn disconnected(&mut self) {
        let had_report = self.loading.is_some() || self.snapshot.is_some();
        self.clear();
        if had_report {
            self.message = Some(
                "Report cleared after connection loss. Reconnect and explicitly Load it again"
                    .into(),
            );
        }
    }

    pub fn path_edited(&mut self) {
        self.clear();
    }

    fn accepts(&self, load: &Load, generation: u64) -> bool {
        generation == load.generation
            && self.path == load.path
            && self.loading.as_ref() == Some(load)
    }

    pub fn visible_cases(&self) -> Vec<usize> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        snapshot
            .report
            .cases
            .iter()
            .enumerate()
            .filter_map(|(index, case)| {
                (self.filter.matches(case.status)
                    && (self.name_filter.is_empty()
                        || case.name.contains(&self.name_filter)
                        || case
                            .classname
                            .as_ref()
                            .is_some_and(|name| name.contains(&self.name_filter))))
                .then_some(index)
            })
            .collect()
    }

    pub fn select(&mut self, index: usize) {
        if self.visible_cases().contains(&index) {
            self.selected = Some(index);
        }
    }

    pub fn filter_changed(&mut self) {
        if self
            .selected
            .is_some_and(|selected| !self.visible_cases().contains(&selected))
        {
            self.selected = None;
        }
    }
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && !path.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        && !path.split('/').any(|part| matches!(part, "" | "." | ".."))
}

fn status_label(status: TestStatus) -> &'static str {
    match status {
        TestStatus::Passed => "Passed",
        TestStatus::Failed => "Failed",
        TestStatus::Error => "Error",
        TestStatus::Skipped => "Skipped",
        TestStatus::Unsupported => "Unsupported / flaky",
    }
}

fn status_color(status: TestStatus) -> egui::Color32 {
    match status {
        TestStatus::Passed => GREEN,
        TestStatus::Failed | TestStatus::Error => RED,
        TestStatus::Skipped => MUTED,
        TestStatus::Unsupported => AMBER,
    }
}

pub(super) fn case_label(case: &TestCase) -> String {
    format!(
        "{} · {} · {:.3} s",
        case.name,
        status_label(case.status),
        case.duration_seconds
    )
}

impl CedarApp {
    fn report_reads_pending(&self) -> usize {
        self.pending
            .values()
            .filter(|job| matches!(job, Job::TestReportRead(_)))
            .count()
    }

    pub(super) fn load_test_report(&mut self) {
        if !self.backend_supports("read") {
            self.test_report.message =
                Some("Connect to a Read-capable workspace before loading a report".into());
            return;
        }
        if !valid_path(&self.test_report.path) {
            self.test_report.clear();
            self.test_report.message = Some("Enter one exact root-relative report path using / separators, without traversal, drive prefixes, empty segments, or control characters".into());
            return;
        }
        if self.report_reads_pending() >= MAX_PENDING_READS {
            self.test_report.message =
                Some("Wait for the outstanding report reads to finish before loading again".into());
            return;
        }
        let Some(id) = self.test_report.next_load.checked_add(1) else {
            self.test_report.message = Some(
                "Report load IDs are exhausted. Restart the frontend before loading again".into(),
            );
            return;
        };
        self.test_report.clear();
        self.test_report.next_load = id;
        let load = Load {
            generation: self.generation,
            id,
            path: self.test_report.path.clone(),
        };
        self.test_report.loading = Some(load.clone());
        if self.request(
            Operation::Read {
                path: load.path.clone(),
            },
            Job::TestReportRead(load),
        ) == 0
        {
            self.test_report.loading = None;
            self.test_report.message =
                Some("Report read could not start. Reconnect and Load it again".into());
        }
    }

    pub(super) fn apply_test_report_read(&mut self, load: Load, result: Result<Payload, String>) {
        if !self.test_report.accepts(&load, self.generation) {
            return;
        }
        self.test_report.loading = None;
        let parsed = (|| {
            let Payload::File {
                path,
                text,
                revision,
            } = result.map_err(|_| {
                "The workspace could not read this report. Check the exact path and retry"
                    .to_owned()
            })?
            else {
                return Err("Unexpected report response; no results were accepted".to_owned());
            };
            if path != load.path {
                return Err(
                    "The agent returned a different report path; no results were accepted"
                        .to_owned(),
                );
            }
            if text.len() > MAX_REPORT_BYTES {
                return Err("Report exceeds the 1 MiB input limit".to_owned());
            }
            // Ordinary Read uses the SHA-256 revision contract. It is a source
            // token bound to these exact bytes, not proof of file freshness or
            // a validation of the peer.
            if revision.len() != 64
                || !revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(
                    "The report response has an invalid source revision; no results were accepted"
                        .to_owned(),
                );
            }
            if revision != format!("{:x}", Sha256::digest(text.as_bytes())) {
                return Err(
                    "The report source revision does not match its text; no results were accepted"
                        .to_owned(),
                );
            }
            let report = parse_report(&text).map_err(|error| error.to_string())?;
            Ok(Snapshot {
                source: load,
                revision,
                report,
            })
        })();
        match parsed {
            Ok(snapshot) => {
                self.cjk_seen |= crate::system_fonts::contains_cjk(&snapshot.source.path)
                    || crate::system_fonts::contains_cjk(&snapshot.report.suite_name)
                    || snapshot.report.cases.iter().any(|case| {
                        crate::system_fonts::contains_cjk(&case.name)
                            || case
                                .classname
                                .as_deref()
                                .is_some_and(crate::system_fonts::contains_cjk)
                            || case.details.iter().any(|detail| {
                                crate::system_fonts::contains_cjk(&detail.text)
                                    || detail
                                        .message
                                        .as_deref()
                                        .is_some_and(crate::system_fonts::contains_cjk)
                                    || detail
                                        .detail_type
                                        .as_deref()
                                        .is_some_and(crate::system_fonts::contains_cjk)
                            })
                    });
                self.test_report.snapshot = Some(snapshot);
                self.test_report.message = None;
            }
            Err(error) => {
                self.test_report.snapshot = None;
                self.test_report.message = Some(format!("Report not loaded: {error}"));
            }
        }
    }

    pub(super) fn test_report_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Read one existing Surefire-style JUnit XML report (single testsuite) from the workspace root").small().color(MUTED));
        let can_load =
            self.backend_supports("read") && self.report_reads_pending() < MAX_PENDING_READS;
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.test_report.path)
                .id(egui::Id::new("test_report_path"))
                .char_limit(MAX_PATH_BYTES)
                .hint_text("target/surefire-reports/TEST-example.xml")
                .desired_width(ui.available_width()),
        );
        if edit.changed() {
            self.test_report.path_edited();
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(can_load, egui::Button::new("Load report"))
                .clicked()
            {
                self.load_test_report();
            }
            let can_refresh = can_load
                && self
                    .test_report
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.source.path == self.test_report.path);
            if ui
                .add_enabled(can_refresh, egui::Button::new("Refresh report"))
                .clicked()
            {
                self.load_test_report();
            }
            if ui.button("Clear report").clicked() {
                self.test_report.clear();
            }
        });
        if let Some(load) = &self.test_report.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Loading {}...", load.path));
            });
        }
        if let Some(message) = &self.test_report.message {
            ui.colored_label(AMBER, message);
        }
        if self.test_report.snapshot.is_none() {
            if self.test_report.loading.is_none() && self.test_report.message.is_none() {
                ui.label(RichText::new("Choose an existing report and click Load report. Results are not loaded automatically").color(MUTED));
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let mut changed = false;
            for filter in [
                Filter::All,
                Filter::Passed,
                Filter::Failed,
                Filter::Error,
                Filter::Skipped,
                Filter::Unsupported,
            ] {
                changed |= ui
                    .selectable_value(&mut self.test_report.filter, filter, filter.label())
                    .changed();
            }
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.test_report.name_filter)
                        .id(egui::Id::new("test_report_filter"))
                        .char_limit(128)
                        .hint_text("Filter name or class")
                        .desired_width(170.0),
                )
                .changed();
            if changed {
                self.test_report.filter_changed();
            }
        });
        let visible = self.test_report.visible_cases();
        let snapshot = self.test_report.snapshot.as_ref().unwrap();
        let report = &snapshot.report;
        let counts = &report.counts;
        let mut select = None;
        let scroll = egui::ScrollArea::vertical().id_salt("test_report_contents").show(ui, |ui| {
            ui.label(RichText::new("Historical report snapshot. It does not establish the current source or test state. Refresh report explicitly to read disk again").small().color(AMBER));
            ui.label(RichText::new(format!("Path: {}", snapshot.source.path)).monospace().small());
            ui.label(RichText::new(format!("Source revision: {}", snapshot.revision)).monospace().small());
            ui.label(format!("{} tests · {} passed · {} failed · {} errors · {} skipped · {} unsupported", counts.total, counts.passed, counts.failed, counts.errors, counts.skipped, counts.unsupported));
            if counts.total == 0 {
                ui.colored_label(AMBER, "No test outcomes in this report; success cannot be inferred");
            } else if counts.unsupported > 0 {
                ui.colored_label(AMBER, "Unsupported or flaky outcomes are not counted as passed");
            }
            let suite = report.duration_seconds.map_or_else(
                || report.suite_name.clone(),
                |seconds| format!("{} · {seconds:.3} s", report.suite_name),
            );
            egui::CollapsingHeader::new(suite)
                .id_salt("test_report_suite")
                .default_open(true)
                .show(ui, |ui| {
                    for index in &visible {
                        let case = &report.cases[*index];
                        if ui.selectable_label(
                            self.test_report.selected == Some(*index),
                            RichText::new(case_label(case)).color(status_color(case.status)),
                        ).clicked() {
                            select = Some(*index);
                        }
                    }
                    if visible.is_empty() {
                        ui.label("No cases match this filter");
                    }
                });
            if let Some(case) = self.test_report.selected.and_then(|index| report.cases.get(index)) {
                ui.separator();
                ui.label(RichText::new(&case.name).strong());
                if let Some(classname) = &case.classname {
                    ui.label(format!("Class: {classname}"));
                }
                ui.label(case_label(case));
                for detail in &case.details {
                    ui.label(RichText::new(&detail.kind).strong());
                    if let Some(kind) = &detail.detail_type {
                        ui.label(format!("Type: {kind}"));
                    }
                    if let Some(message) = &detail.message {
                        ui.label(message);
                    }
                    ui.label(RichText::new(&detail.text).monospace());
                }
                if case.details.is_empty() {
                    ui.label(RichText::new("No failure or skip detail recorded").color(MUTED));
                }
            }
        });
        #[cfg(test)]
        crate::workspace_access_tests::record_report_scroll(
            ui.ctx(),
            scroll.id,
            scroll.state.offset,
        );
        #[cfg(not(test))]
        let _ = scroll;
        if let Some(index) = select {
            self.test_report.select(index);
        }
    }
}
