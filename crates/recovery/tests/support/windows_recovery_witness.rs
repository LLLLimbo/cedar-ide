//! NONSHIPPING single negative admission witness; no Store access.
use super::admission::{Admitted, Error, ModelSink, Observation, Operation, Queries, Role};
use super::policy::Rejection;

#[derive(serde::Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub witness: &'static str,
    pub fixture_scope: &'static str,
    pub atomic_directory_creation: bool,
    pub ancestor_namespace_verified: bool,
    pub category: &'static str,
    pub observation_attempts: usize,
    pub observations_succeeded: usize,
    pub admission_attempts: usize,
    pub fake_read_calls: usize,
    pub fake_payload_calls: usize,
    pub fake_remove_calls: usize,
    pub generated_directories: usize,
    pub generated_files: usize,
    pub handles_dropped_before_cleanup: bool,
    pub empty_verified: bool,
    pub cleanup_complete: bool,
}
impl Default for Receipt {
    fn default() -> Self {
        Self {
            schema_version: 1,
            witness: "windows_local_app_data_owner_rejection_witness",
            fixture_scope: "controlled_fresh_runner",
            atomic_directory_creation: false,
            ancestor_namespace_verified: false,
            category: "setup_error",
            observation_attempts: 0,
            observations_succeeded: 0,
            admission_attempts: 0,
            fake_read_calls: 0,
            fake_payload_calls: 0,
            fake_remove_calls: 0,
            generated_directories: 0,
            generated_files: 0,
            handles_dropped_before_cleanup: false,
            empty_verified: false,
            cleanup_complete: false,
        }
    }
}
impl Receipt {
    pub fn succeeded(&self) -> bool {
        self.category == "owner_mismatch"
            && self.observation_attempts == 1
            && self.observations_succeeded == 1
            && self.admission_attempts == 1
            && self.fake_read_calls == 0
            && self.fake_payload_calls == 0
            && self.fake_remove_calls == 0
            && self.generated_directories == 1
            && self.generated_files == 1
            && self.handles_dropped_before_cleanup
            && self.empty_verified
            && self.cleanup_complete
    }
}

struct Counted<'a, Q> {
    queries: &'a mut Q,
    receipt: &'a mut Receipt,
}
impl<H, Q: Queries<H>> Queries<H> for Counted<'_, Q> {
    fn observe(&mut self, handle: &H) -> Result<Observation, Error> {
        self.receipt.observation_attempts += 1;
        let result = self.queries.observe(handle);
        if result.is_ok() {
            self.receipt.observations_succeeded += 1;
        }
        result
    }
}

#[derive(Default)]
struct CountingFakeSink {
    read: usize,
    payload: usize,
    remove: usize,
}
impl<H> ModelSink<H> for CountingFakeSink {
    fn call(&mut self, _: &H, operation: Operation) {
        // Memory counters only; never content access, writes, or removal.
        match operation {
            Operation::Read => self.read += 1,
            Operation::Payload => self.payload += 1,
            Operation::Remove => self.remove += 1,
        }
    }
}

/// Exact ExistingRecord admission followed by a memory-only modeled Read.
/// Initially admitted fixtures cannot establish the intended rejection, even
/// when the later exercise revalidation returns OwnerMismatch.
/// No observation is fabricated, cached, altered, or retried by this wrapper.
pub fn reject_before_read<H>(handle: H, queries: &mut impl Queries<H>, receipt: &mut Receipt) {
    receipt.admission_attempts += 1;
    let mut sink = CountingFakeSink::default();
    let mut initially_admitted = false;
    let result = {
        let mut counted = Counted { queries, receipt };
        Admitted::admit(handle, Role::ExistingRecord, &mut counted).and_then(|admitted| {
            initially_admitted = true;
            admitted.exercise(Operation::Read, &mut counted, &mut sink)
        })
    };
    receipt.fake_read_calls = sink.read;
    receipt.fake_payload_calls = sink.payload;
    receipt.fake_remove_calls = sink.remove;
    receipt.category = if initially_admitted {
        "intended_negative_fixture_unavailable"
    } else {
        match result {
            Err(Error::Policy(Rejection::OwnerMismatch)) => "owner_mismatch",
            Err(Error::Policy(_) | Error::Parse(_)) => "unexpected_rejection",
            Err(_) => "query_error",
            Ok(_) => "intended_negative_fixture_unavailable",
        }
    };
    // Both failed admission and unexpected success release the owned handle.
}
