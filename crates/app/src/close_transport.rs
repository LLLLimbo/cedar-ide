//! App-lifetime local-owner receipts are separate from a cancellable exit intent.
//! Neither a missing Worker, elapsed time nor a peer message is cleanup proof.
use super::*;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const OBSERVATION: Duration = Duration::from_secs(5);
const MAX_REASON_BYTES: usize = 1024;
pub(crate) const WAITING: &str =
    "Waiting for local connection cleanup before quitting. Drafts are retained.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnerTicket {
    id: u64,
    pub(crate) generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnerOutcome {
    NoChild,
    Verified,
    Unverified(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnerReceipt {
    pub(crate) ticket: OwnerTicket,
    pub(crate) outcome: OwnerOutcome,
}
#[derive(Debug)]
struct PendingOwner {
    generation: u64,
    retired: bool,
}
#[derive(Debug)]
struct UnverifiedSummary {
    count: u64,
    first: Option<OwnerTicket>,
    latest: Option<OwnerTicket>,
    first_reason: String,
    latest_reason: String,
}
#[derive(Default, Debug)]
pub(crate) struct OwnerLedger {
    next: u64,
    pending: BTreeMap<u64, PendingOwner>,
    // Never cleared by detail truncation, a duplicate, reconnect or Keep editing.
    blocked: bool,
    unverified: Option<UnverifiedSummary>,
}
impl OwnerLedger {
    pub(crate) fn register(&mut self, generation: u64) -> Result<OwnerTicket, String> {
        let id = self.next.checked_add(1).ok_or_else(|| {
            "Connection owner identifiers are exhausted; no new connection was started".to_string()
        })?;
        self.next = id;
        self.pending.insert(
            id,
            PendingOwner {
                generation,
                retired: false,
            },
        );
        Ok(OwnerTicket { id, generation })
    }
    pub(crate) fn retire(&mut self, ticket: OwnerTicket) {
        // A settled active registration capability legitimately outlives pending.
        if let Some(owner) = self.pending.get_mut(&ticket.id) {
            if owner.generation == ticket.generation {
                owner.retired = true;
            }
        }
    }
    pub(crate) fn apply(&mut self, receipt: OwnerReceipt) -> bool {
        if !self
            .pending
            .get(&receipt.ticket.id)
            .is_some_and(|owner| owner.generation == receipt.ticket.generation)
        {
            return false;
        }
        self.pending.remove(&receipt.ticket.id);
        if let OwnerOutcome::Unverified(reason) = receipt.outcome {
            self.note_unverified(Some(receipt.ticket), reason);
        }
        true
    }
    fn note_unverified(&mut self, ticket: Option<OwnerTicket>, mut reason: String) {
        let mut end = reason.len().min(MAX_REASON_BYTES);
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        reason.truncate(end);
        self.blocked = true;
        match &mut self.unverified {
            Some(summary) => {
                summary.count = summary.count.saturating_add(1);
                summary.latest = ticket;
                summary.latest_reason = reason;
            }
            None => {
                self.unverified = Some(UnverifiedSummary {
                    count: 1,
                    first: ticket,
                    latest: ticket,
                    first_reason: reason.clone(),
                    latest_reason: reason,
                })
            }
        }
    }
    fn channel_lost(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (id, owner) in pending {
            self.note_unverified(
                Some(OwnerTicket {
                    id,
                    generation: owner.generation,
                }),
                "Local owner completion channel disconnected without a receipt".into(),
            );
        }
    }
    pub(crate) fn all_verified(&self) -> bool {
        self.pending.is_empty() && !self.blocked
    }
    fn message(&self, overdue: bool) -> String {
        let retired = self.pending.values().filter(|owner| owner.retired).count();
        let pending = format!(
            "{} pending receipt(s), including {retired} retired owner(s)",
            self.pending.len()
        );
        if let Some(summary) = &self.unverified {
            let lineage = summary.first.map_or_else(
                || "unregistered owner".into(),
                |ticket| format!("owner {} / generation {}", ticket.id, ticket.generation),
            );
            let latest = summary.latest.map_or_else(
                || "unknown owner".into(),
                |ticket| format!("owner {} / generation {}", ticket.id, ticket.generation),
            );
            return format!("Local connection cleanup is unverified ({} outcome(s), first {lineage}, latest {latest}; {pending}). Quitting is blocked; Keep editing retains your drafts. {}", summary.count, if summary.first_reason == summary.latest_reason { summary.latest_reason.clone() } else { format!("First: {} Latest: {}", summary.first_reason, summary.latest_reason) });
        }
        if self.blocked {
            return format!("Local connection cleanup is unverified; {pending}. Quitting is blocked; Keep editing retains your drafts.");
        }
        format!("Waiting for {} local connection cleanup receipt(s), including {retired} retired owner(s). {}", self.pending.len(), if overdue { "Completion has not been verified. Keep editing retains your drafts; elapsed time is not cleanup proof." } else { "Keep editing retains your drafts." })
    }
}

pub(crate) struct CloseIntent {
    pub(crate) serial: u64,
    guard: recovery::CloseGuard,
    ticket: recovery_actor::Ticket,
    started: Instant,
}

impl CedarApp {
    pub(crate) fn connection_admission_problem(&self) -> Option<&'static str> {
        if self.connection_generation_exhausted {
            Some("Connection generations are exhausted. No new connection can be started; local drafts remain available.")
        } else if self.state == ConnectionState::Disconnecting {
            Some(disconnect::WAITING)
        } else if self.close_after_language_stop
            || self.recovery.closing.is_some()
            || self.transport_close.is_some()
            || self.state == ConnectionState::ClosingTransport
        {
            Some("Finish closing or choose Keep editing before starting another connection.")
        } else {
            None
        }
    }
    pub(crate) fn ordinary_worker_ui_blocked(&self) -> bool {
        self.connection_generation_exhausted || self.state == ConnectionState::ClosingTransport
    }
    pub(crate) fn retire_current_worker(&mut self) {
        if self.worker.as_ref().is_some_and(|worker| {
            self.active_owner.is_none() || worker.owner_ticket() != self.active_owner
        }) {
            self.owner_ledger.note_unverified(
                None,
                "Active connection did not match its registered local owner".into(),
            );
            self.unverified_local_close = true;
        }
        if let Some(ticket) = self.active_owner.take() {
            self.owner_ledger.retire(ticket);
        }
        self.worker = None;
    }
    pub(crate) fn retire_ui_generation(&mut self) {
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
        } else {
            // Dropping the handle does not make same-generation late UI replies safe.
            self.connection_generation_exhausted = true;
            self.error = Some("Connection generations are exhausted. Retired replies are ignored; local drafts remain available.".into());
        }
    }
    pub(crate) fn apply_owner_receipt(&mut self, receipt: OwnerReceipt) {
        self.owner_ledger.apply(receipt);
        if self.owner_ledger.blocked {
            self.unverified_local_close = true;
        }
        // Do not clear active_owner: its registration capability may outlive a
        // settled pending entry until the separate legacy Closed event is drained.
    }
    pub(crate) fn owner_channel_lost(&mut self) {
        self.owner_ledger.channel_lost();
        if self.owner_ledger.blocked {
            self.unverified_local_close = true;
        }
    }
    pub(crate) fn transport_close_message(&self) -> String {
        self.owner_ledger.message(
            self.transport_close
                .as_ref()
                .is_some_and(|intent| intent.started.elapsed() >= OBSERVATION),
        )
    }
    pub(crate) fn finish_transport_close_frame(&mut self, ctx: &egui::Context) {
        if self.allow_close {
            return;
        }
        if ctx.will_discard() || ctx.output(|output| output.requested_discard()) {
            if self.recovery.closing.is_some() || self.transport_close.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.request_repaint();
            }
            return;
        }
        let confirmed = self
            .recovery
            .closing
            .as_ref()
            .and_then(|close| match close.phase {
                recovery::ClosePhase::Confirmed { ticket } => Some((close.guard.clone(), ticket)),
                _ => None,
            });
        let Some((guard, ticket)) = confirmed else {
            if self.transport_close.is_some() {
                self.keep_editing_recovery(ctx);
            }
            return;
        };
        let same_intent = self.transport_close.as_ref().is_none_or(|intent| {
            intent.serial == self.next_transport_close
                && intent.guard == guard
                && intent.ticket == ticket
        });
        if !same_intent
            || guard != self.recovery_close_guard()
            || self.mutation_pending()
            || self.language.running
            || self.language_busy()
            || self.state == ConnectionState::Disconnecting
            || !self.guard_run_transition(run_ui::Transition::Close)
        {
            self.keep_editing_recovery(ctx);
            self.notice =
                "Close approval changed. Review the retained drafts before quitting.".into();
            return;
        }
        if self.transport_close.is_none() {
            let Some(serial) = self.next_transport_close.checked_add(1) else {
                self.keep_editing_recovery(ctx);
                self.error =
                    Some("Close identifiers are exhausted; exit approval was canceled".into());
                return;
            };
            self.next_transport_close = serial;
            self.transport_close = Some(CloseIntent {
                serial,
                guard: guard.clone(),
                ticket,
                started: Instant::now(),
            });
            self.state = ConnectionState::ClosingTransport;
            self.retire_current_worker();
            // Mutating requests were excluded above. Retire only remaining read
            // and navigation intents, keeping the approved workspace/draft guard.
            self.pending.clear();
            self.navigation_changed();
            self.workspace_access = workspace_access::Access::default();
            self.connecting_form = None;
            self.recovery.restoring_generation = None;
            self.recovery.visible = true;
            self.notice = WAITING.into();
            ctx.request_repaint();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if !self.owner_ledger.all_verified() {
            self.recovery.visible = true;
            self.notice = self.transport_close_message();
            if let Some(intent) = &self.transport_close {
                ctx.request_repaint_after(
                    OBSERVATION
                        .saturating_sub(intent.started.elapsed())
                        .max(Duration::from_millis(50)),
                );
            }
            return;
        }
        // This is the only final Close emitter, after every frame finisher and
        // only on a stable pass. A receipt does not approve changed editor text.
        if guard != self.recovery_close_guard()
            || self.mutation_pending()
            || self.language.running
            || self.language_busy()
            || !self.guard_run_transition(run_ui::Transition::Close)
        {
            self.keep_editing_recovery(ctx);
            return;
        }
        self.transport_close = None;
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_owner_and_sticky_uncertainty_are_independent_of_current_success() {
        let mut ledger = OwnerLedger::default();
        let old = ledger.register(1).unwrap();
        ledger.retire(old);
        let current = ledger.register(2).unwrap();
        assert!(ledger.apply(OwnerReceipt {
            ticket: current,
            outcome: OwnerOutcome::Verified
        }));
        assert!(!ledger.all_verified());
        assert!(ledger.apply(OwnerReceipt {
            ticket: old,
            outcome: OwnerOutcome::Unverified("wait failed".into())
        }));
        assert!(!ledger.all_verified());
        assert!(!ledger.apply(OwnerReceipt {
            ticket: old,
            outcome: OwnerOutcome::Verified
        }));
        let next = ledger.register(3).unwrap();
        ledger.retire(next);
        let message = ledger.message(false);
        assert!(message.contains("unverified"));
        assert!(message.contains("1 pending receipt(s)"));
        assert!(message.contains("1 retired owner(s)"));
        ledger.apply(OwnerReceipt {
            ticket: next,
            outcome: OwnerOutcome::NoChild,
        });
        assert!(!ledger.all_verified());
    }
    #[test]
    fn unknown_generation_duplicate_and_missing_receipts_never_clear_pending() {
        let mut ledger = OwnerLedger::default();
        let ticket = ledger.register(7).unwrap();
        for unknown in [
            OwnerTicket {
                id: ticket.id + 1,
                ..ticket
            },
            OwnerTicket {
                generation: 8,
                ..ticket
            },
        ] {
            assert!(!ledger.apply(OwnerReceipt {
                ticket: unknown,
                outcome: OwnerOutcome::Verified
            }));
            assert!(!ledger.all_verified());
        }
        assert!(ledger.apply(OwnerReceipt {
            ticket,
            outcome: OwnerOutcome::NoChild
        }));
        ledger.retire(ticket);
        assert!(ledger.all_verified());
        assert!(!ledger.apply(OwnerReceipt {
            ticket,
            outcome: OwnerOutcome::Unverified("duplicate".into())
        }));
        assert!(ledger.all_verified());
    }
    #[test]
    fn channel_loss_is_unverified_and_summary_is_bounded_without_eviction() {
        let mut ledger = OwnerLedger::default();
        for generation in 0..100 {
            ledger.register(generation).unwrap();
        }
        assert_eq!(ledger.pending.len(), 100);
        ledger.channel_lost();
        assert!(ledger.pending.is_empty());
        assert!(!ledger.all_verified());
        ledger.note_unverified(None, "草".repeat(2000));
        let summary = ledger.unverified.as_ref().unwrap();
        assert_eq!(summary.count, 101);
        assert!(summary.latest_reason.len() <= MAX_REASON_BYTES);
        assert!(std::str::from_utf8(summary.latest_reason.as_bytes()).is_ok());
        ledger.unverified = None; // Even removed diagnostics cannot remove the blocker.
        assert!(!ledger.all_verified());
    }
    #[test]
    fn successful_owner_history_does_not_grow_and_id_exhaustion_admits_nothing() {
        let mut ledger = OwnerLedger::default();
        for generation in 0..1000 {
            let ticket = ledger.register(generation).unwrap();
            assert!(ledger.apply(OwnerReceipt {
                ticket,
                outcome: OwnerOutcome::Verified
            }));
        }
        assert!(ledger.all_verified());
        assert!(ledger.pending.is_empty());
        ledger.next = u64::MAX;
        assert!(ledger.register(1001).is_err());
        assert!(ledger.all_verified());
    }
}
