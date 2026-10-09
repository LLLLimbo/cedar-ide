//! Deliberate idle connection release. This is a local ownership transition,
//! not a remote process-cleanup protocol or a retry of any workspace operation.
use super::*;

pub(super) const WAITING: &str =
    "Disconnecting: waiting for local connection cleanup. Drafts are retained.";
pub(super) const UNVERIFIED: &str = "An earlier local connection cleanup was not verified. Its process may still exist; reconnecting does not verify that cleanup.";

impl CedarApp {
    pub(super) fn disconnect_problem(&self) -> Option<&'static str> {
        if self.state == ConnectionState::Disconnecting {
            return Some(WAITING);
        }
        if !self.ready() || self.worker.is_none() {
            return Some("Connect to a workspace before disconnecting it.");
        }
        if self.confirm.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.recovery.closing.is_some()
            || self.recovery.restoring_generation.is_some()
        {
            return Some("Finish the current close or recovery transition before disconnecting.");
        }
        if !self.run_state.idle_for_disconnect() {
            return Some("Stop or cancel the command and wait for a verified terminal outcome before disconnecting. Unknown command outcomes cannot be dismissed by Disconnect.");
        }
        if !self.language.idle_for_disconnect() {
            return Some("Stop the language session and verify its cleanup before disconnecting.");
        }
        if self.documents.iter().any(|doc| doc.saving)
            || self.pending.values().any(|job| match job {
                Job::Save { .. } | Job::Git | Job::GitRead(_) | Job::Run(_) | Job::Language(_) => {
                    true
                }
                Job::List { .. }
                | Job::TreeList { .. }
                | Job::Open { .. }
                | Job::LanguageOpen { .. }
                | Job::BuildProblemOpen { .. }
                | Job::JavaImplementationOpen { .. }
                | Job::JavaTypeOpen { .. }
                | Job::Search { .. }
                | Job::ProfilesLoad { .. }
                | Job::TestReportRead(_)
                | Job::DiskReview { .. }
                | Job::InterruptedSaveCheck { .. } => false,
            })
        {
            return Some("Wait for the save, Git, command or language request to finish before disconnecting.");
        }
        None
    }

    pub(super) fn disconnect_idle(&mut self) {
        if let Some(problem) = self.disconnect_problem() {
            self.error = Some(problem.into());
            return;
        }
        // Intent starts here, after this frame's ordered response drain. Never
        // reclassify an acknowledged save or preempt an earlier completed read.
        self.state = ConnectionState::Disconnecting;
        self.navigation_changed();
        self.workspace_access = workspace_access::Access::default();
        for doc in &mut self.documents {
            doc.jump_to = None;
        }
        self.reset_git(false);
        self.run_state.disconnected();
        self.test_report.disconnected();
        self.profiles.disconnected();
        self.recovery.restoring_generation = None;
        self.connecting_form = None;
        self.agent_info = None;
        self.language.reset();
        self.pending.clear();
        self.explorer.reset_connection();
        self.disk_review.outstanding = None;
        self.interrupted_save_check.reset();
        self.error = None;
        self.notice = WAITING.into();
        // The existing worker owns Client and its eventual cleanup receipt.
        // None here means cancellation was handed off, never that cleanup passed.
        self.worker = None;
    }

    pub(super) fn connection_closed(&mut self, generation: u64, result: Result<(), String>) {
        if generation != self.generation {
            return;
        }
        if self.state == ConnectionState::Disconnecting {
            match result {
                Ok(()) => {
                    self.state = ConnectionState::Disconnected;
                    self.notice = if self.active_form.as_ref().is_some_and(|form| form.ssh) {
                        "Disconnected · local SSH client closed; remote cleanup is not verified · drafts retained".into()
                    } else {
                        "Disconnected · local connection cleanup confirmed · drafts retained".into()
                    };
                    self.error = None;
                }
                Err(_) => {
                    self.state = ConnectionState::CleanupUnverified;
                    self.unverified_local_close = true;
                    self.notice =
                        "Connection released · local cleanup unverified · drafts retained".into();
                    self.error = Some(UNVERIFIED.into());
                }
            }
        } else if self.ready()
            || (self.state == ConnectionState::Connecting && self.connecting_form.is_some())
        {
            // A worker can unwind or exit without a normal request error. Its
            // terminal notice must not leave an apparently usable connection.
            self.disconnected(
                "The connection worker exited. Your drafts are retained; no operation was retried."
                    .into(),
            );
            if result.is_err() {
                self.unverified_local_close = true;
            }
        }
        // Duplicates, initial-connect cancellation and old fault notifications
        // do not overwrite an already selected terminal state or a later session.
    }
}
