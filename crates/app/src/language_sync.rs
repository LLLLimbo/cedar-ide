//! Deterministic debounce bookkeeping; clocks are provided by the native event loop.
use std::collections::HashMap;
pub const DEBOUNCE_SECONDS: f64 = 0.35;
#[derive(Clone, Debug)]
pub struct Acknowledged {
    pub version: i32,
    pub edit_version: u64,
    pub uri: String,
}
#[derive(Default)]
pub struct SyncTracker {
    pub opened: HashMap<u64, Acknowledged>,
    observed: HashMap<u64, (u64, f64)>,
    failed: HashMap<u64, u64>,
}
impl SyncTracker {
    pub fn clear(&mut self) {
        self.opened.clear();
        self.observed.clear();
        self.failed.clear();
    }
    pub fn observe(&mut self, document: u64, edit_version: u64, now: f64) {
        match self.observed.get_mut(&document) {
            Some((observed, changed)) if *observed != edit_version => {
                *observed = edit_version;
                *changed = now;
            }
            None => {
                self.observed
                    .insert(document, (edit_version, now - DEBOUNCE_SECONDS));
            }
            _ => {}
        }
    }
    pub fn synced(&self, document: u64, edit_version: u64) -> bool {
        self.opened
            .get(&document)
            .is_some_and(|ack| ack.edit_version == edit_version)
    }
    pub fn deadline(&self, document: u64, edit_version: u64) -> Option<f64> {
        if self.synced(document, edit_version) || self.failed.get(&document) == Some(&edit_version)
        {
            return None;
        }
        self.observed
            .get(&document)
            .filter(|(observed, _)| *observed == edit_version)
            .map(|(_, changed)| changed + DEBOUNCE_SECONDS)
    }
    pub fn next_version(&self, document: u64) -> Option<i32> {
        self.opened
            .get(&document)
            .map_or(Some(1), |ack| ack.version.checked_add(1))
    }
    pub fn acknowledge(&mut self, document: u64, ack: Acknowledged) {
        if self
            .opened
            .get(&document)
            .is_none_or(|old| ack.version > old.version)
        {
            self.opened.insert(document, ack);
        }
        self.failed.remove(&document);
    }
    pub fn fail(&mut self, document: u64, version: u64) {
        self.failed.insert(document, version);
    }
    pub fn retry(&mut self, document: u64) {
        self.failed.remove(&document);
    }
    pub fn close(&mut self, document: u64) {
        self.opened.remove(&document);
        self.observed.remove(&document);
        self.failed.remove(&document);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn debounce_coalesces_changes_and_ack_never_covers_newer_typing() {
        let mut tracker = SyncTracker::default();
        tracker.observe(1, 0, 1.0);
        assert!(tracker.deadline(1, 0).unwrap() <= 1.0);
        tracker.acknowledge(
            1,
            Acknowledged {
                version: 1,
                edit_version: 0,
                uri: "file:///f".into(),
            },
        );
        tracker.observe(1, 1, 2.0);
        tracker.observe(1, 2, 2.1);
        assert!((tracker.deadline(1, 2).unwrap() - 2.45).abs() < 0.00001);
        tracker.acknowledge(
            1,
            Acknowledged {
                version: 2,
                edit_version: 1,
                uri: "file:///f".into(),
            },
        );
        assert!(!tracker.synced(1, 2));
        assert_eq!(tracker.next_version(1), Some(3));
    }
    #[test]
    fn failures_do_not_create_tight_retry_loops() {
        let mut tracker = SyncTracker::default();
        tracker.observe(1, 2, 0.0);
        tracker.fail(1, 2);
        assert!(tracker.deadline(1, 2).is_none());
        tracker.observe(1, 3, 1.0);
        assert!(tracker.deadline(1, 3).is_some());
        tracker.clear();
        assert!(!tracker.synced(1, 2));
    }
}
