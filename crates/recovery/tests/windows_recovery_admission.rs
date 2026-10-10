//! NONSHIPPING: portable admission/model tests and Windows synthetic buffers.
//! One separately opted-in native rejection witness remains ignored.
#![allow(dead_code)]
#[path = "support/windows_recovery_admission.rs"]
mod admission;
#[path = "support/windows_privacy_policy.rs"]
mod policy;
#[cfg(windows)]
#[path = "support/windows_recovery_queries.rs"]
mod queries;
#[path = "support/windows_recovery_witness.rs"]
mod witness;
#[cfg(windows)]
#[path = "support/windows_recovery_witness_native.rs"]
mod witness_native;

#[cfg(windows)]
#[test]
#[ignore = "NONSHIPPING single explicit owner rejection witness; fixed 60-second driver only"]
fn windows_local_app_data_owner_rejection_witness() {
    let receipt = witness_native::run();
    println!(
        "CEDAR_RECOVERY_ADMISSION_WITNESS={}",
        serde_json::to_string(&receipt).expect("fixed receipt serialization")
    );
    assert!(
        receipt.succeeded(),
        "native admission witness did not establish the intended rejection"
    );
}

#[cfg(test)]
mod tests {
    use super::admission::*;
    use super::policy::{Identity, ObjectFacts, Rejection};
    use std::cell::Cell;
    use std::rc::Rc;
    const USER: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 42, 0, 0, 0];
    fn descriptor() -> Vec<u8> {
        let mut bytes = vec![1, 0, 4, 128];
        bytes.extend_from_slice(&20u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.extend_from_slice(USER);
        bytes.extend_from_slice(&[2, 0, 28, 0, 1, 0, 0, 0, 0, 0, 20, 0]);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(USER);
        bytes
    }
    fn identity() -> Identity {
        Identity {
            volume: 0x100000001,
            file: [7; 16],
        }
    }
    struct Handle {
        label: u32,
        drops: Rc<Cell<usize>>,
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    struct Fake {
        call: usize,
        fault_at: usize,
        fault: Fault,
        empty: bool,
        directory: bool,
    }
    #[derive(Clone, Copy)]
    enum Fault {
        None,
        Query,
        Owner,
        Dacl,
        Disk,
        Reparse,
        Type,
        Links,
        AclSupport,
        FileId,
        Volume,
        Token,
    }
    impl Queries<Handle> for Fake {
        fn observe(&mut self, handle: &Handle) -> Result<Observation, Error> {
            assert_eq!(handle.label, 17);
            self.call += 1;
            let mut observation = Observation {
                identity: identity(),
                facts: ObjectFacts {
                    disk: true,
                    directory: self.directory,
                    reparse: false,
                    links: 1,
                    persistent_acls: true,
                    empty: self.empty,
                },
                descriptor: descriptor(),
                user: USER.to_vec(),
            };
            if self.call == self.fault_at {
                match self.fault {
                    Fault::None => {}
                    Fault::Query => return Err(Error::Query),
                    Fault::Owner => observation.descriptor[28] ^= 1,
                    Fault::Dacl => observation.descriptor[56] ^= 1,
                    Fault::Disk => observation.facts.disk = false,
                    Fault::Reparse => observation.facts.reparse = true,
                    Fault::Type => observation.facts.directory = !self.directory,
                    Fault::Links => observation.facts.links = 2,
                    Fault::AclSupport => observation.facts.persistent_acls = false,
                    Fault::FileId => observation.identity.file[15] ^= 1,
                    Fault::Volume => observation.identity.volume ^= 1 << 40,
                    Fault::Token => observation.user[8] ^= 1,
                }
            }
            Ok(observation)
        }
    }
    #[derive(Default)]
    struct Sink {
        calls: [usize; 3],
    }
    impl ModelSink<Handle> for Sink {
        fn call(&mut self, handle: &Handle, operation: Operation) {
            assert_eq!(handle.label, 17); // The sink receives precisely the owned, observed handle.
            self.calls[match operation {
                Operation::Payload => 0,
                Operation::Read => 1,
                Operation::Remove => 2,
            }] += 1;
        }
    }
    fn setup(empty: bool) -> (Handle, Fake, Rc<Cell<usize>>) {
        let drops = Rc::new(Cell::new(0));
        (
            Handle {
                label: 17,
                drops: drops.clone(),
            },
            Fake {
                call: 0,
                fault_at: 0,
                fault: Fault::None,
                empty,
                directory: false,
            },
            drops,
        )
    }
    #[test]
    fn every_rejection_and_query_uncertainty_keeps_all_sinks_zero_and_closes_owner() {
        for (role, operation) in [
            (Role::Temporary, Operation::Payload),
            (Role::ExistingRecord, Operation::Read),
            (Role::ExistingRecord, Operation::Remove),
        ] {
            for fault in [
                Fault::Query,
                Fault::Owner,
                Fault::Dacl,
                Fault::Disk,
                Fault::Reparse,
                Fault::Type,
                Fault::Links,
                Fault::AclSupport,
                Fault::FileId,
                Fault::Volume,
                Fault::Token,
            ] {
                for fault_at in [2, 3] {
                    let (handle, mut fake, drops) = setup(true);
                    fake.fault = fault;
                    fake.fault_at = fault_at;
                    let mut sink = Sink::default();
                    let result = Admitted::admit(handle, role, &mut fake)
                        .and_then(|admitted| admitted.exercise(operation, &mut fake, &mut sink));
                    assert!(result.is_err());
                    assert_eq!(sink.calls, [0; 3]);
                    assert_eq!(drops.get(), 1);
                }
            }
        }
    }
    #[test]
    fn initial_observation_success_does_not_admit_rejected_policy() {
        for fault in [
            Fault::Owner,
            Fault::Dacl,
            Fault::Disk,
            Fault::Reparse,
            Fault::Type,
            Fault::Links,
            Fault::AclSupport,
            Fault::Query,
        ] {
            let (handle, mut fake, drops) = setup(true);
            fake.fault = fault;
            fake.fault_at = 1;
            assert!(Admitted::admit(handle, Role::Temporary, &mut fake).is_err());
            assert_eq!(drops.get(), 1);
            assert_eq!(fake.call, 1);
        }
    }
    #[test]
    fn single_witness_requires_exact_owner_rejection_and_releases_handle() {
        for (fault, category, observations, successes, reads) in [
            (Fault::Owner, "owner_mismatch", 1, 1, 0),
            (Fault::Query, "query_error", 1, 0, 0),
            (Fault::Dacl, "unexpected_rejection", 1, 1, 0),
            (
                Fault::None,
                "intended_negative_fixture_unavailable",
                3,
                3,
                1,
            ),
        ] {
            let (handle, mut fake, drops) = setup(true);
            fake.fault = fault;
            fake.fault_at = 1;
            let mut receipt = super::witness::Receipt::default();
            super::witness::reject_before_read(handle, &mut fake, &mut receipt);
            assert_eq!(receipt.category, category);
            assert_eq!(receipt.observation_attempts, observations);
            assert_eq!(receipt.observations_succeeded, successes);
            assert_eq!(receipt.admission_attempts, 1);
            assert_eq!(
                (
                    receipt.fake_read_calls,
                    receipt.fake_payload_calls,
                    receipt.fake_remove_calls
                ),
                (reads, 0, 0)
            );
            assert_eq!(drops.get(), 1);
            assert!(!receipt.succeeded()); // Cleanup evidence cannot be invented.
        }
    }
    #[test]
    fn late_owner_rejection_or_query_error_cannot_masquerade_as_initial_rejection() {
        for (fault, successes) in [(Fault::Owner, 3), (Fault::Query, 2)] {
            let (handle, mut fake, drops) = setup(true);
            fake.fault = fault;
            fake.fault_at = 3;
            let mut receipt = super::witness::Receipt::default();
            super::witness::reject_before_read(handle, &mut fake, &mut receipt);
            assert_eq!(receipt.category, "intended_negative_fixture_unavailable");
            assert_eq!(receipt.observation_attempts, 3);
            assert_eq!(receipt.observations_succeeded, successes);
            assert_eq!(receipt.fake_read_calls, 0);
            assert_eq!(receipt.fake_payload_calls, 0);
            assert_eq!(receipt.fake_remove_calls, 0);
            assert_eq!(drops.get(), 1);
            assert!(!receipt.succeeded());
        }
    }
    #[test]
    fn nonempty_existing_records_are_allowed_but_lock_and_temp_are_rejected() {
        for role in [Role::Lock, Role::Temporary] {
            let (handle, mut fake, drops) = setup(false);
            assert!(matches!(
                Admitted::admit(handle, role, &mut fake),
                Err(Error::Policy(Rejection::NonemptyFile))
            ));
            assert_eq!(drops.get(), 1);
        }
        for operation in [Operation::Read, Operation::Remove] {
            let (handle, mut fake, drops) = setup(false);
            let mut sink = Sink::default();
            Admitted::admit(handle, Role::ExistingRecord, &mut fake)
                .unwrap()
                .exercise(operation, &mut fake, &mut sink)
                .unwrap();
            assert_eq!(sink.calls.iter().sum::<usize>(), 1);
            assert_eq!(drops.get(), 1);
        }
    }
    #[test]
    fn typed_roles_restrict_sinks_and_require_correct_object_type() {
        for role in [
            Role::Ancestor,
            Role::Root,
            Role::RecordDirectory,
            Role::Lock,
            Role::Temporary,
            Role::ExistingRecord,
        ] {
            let (handle, mut fake, drops) = setup(true);
            fake.directory = role.directory();
            let mut sink = Sink::default();
            let result = Admitted::admit(handle, role, &mut fake).unwrap().exercise(
                Operation::Payload,
                &mut fake,
                &mut sink,
            );
            assert_eq!(result.is_ok(), role == Role::Temporary);
            assert_eq!(sink.calls[0], usize::from(role == Role::Temporary));
            assert_eq!(drops.get(), 1);
        }
    }
    #[test]
    fn uncertain_mutation_never_becomes_success_or_a_retry_permission() {
        for reopened in [
            Err(Error::Query),
            Ok(Identity {
                file: [0; 16],
                ..identity()
            }),
            Ok(Identity {
                volume: 1,
                ..identity()
            }),
        ] {
            let mut state = Mutation::NotAttempted;
            state.begin().unwrap();
            state.observe_publication(identity(), reopened);
            state.acknowledge_durability(true);
            assert_eq!(state, Mutation::Uncertain);
            assert_eq!(state.begin(), Err(Error::MutationUncertain));
        }
        let mut state = Mutation::NotAttempted;
        state.begin().unwrap();
        state.observe_publication(identity(), Ok(identity()));
        state.acknowledge_durability(false);
        assert_eq!(state, Mutation::PublishedObserved);
        assert!(state.begin().is_err());
        state.acknowledge_durability(true);
        assert_eq!(state, Mutation::DurabilityAcknowledged);
    }
    #[test]
    fn remove_attempt_and_absence_observation_do_not_imply_committed_success() {
        let (handle, mut fake, _) = setup(false);
        let mut sink = Sink::default();
        let attempted = Admitted::admit(handle, Role::ExistingRecord, &mut fake)
            .unwrap()
            .exercise(Operation::Remove, &mut fake, &mut sink)
            .unwrap();
        assert_eq!(attempted, Attempted::OutcomeUnknown);
        for absent in [Err(Error::Query), Ok(false), Ok(true)] {
            let mut removal = Removal::NotAttempted;
            removal.begin().unwrap();
            removal.observe_absence(absent);
            assert_eq!(
                removal,
                if absent == Ok(true) {
                    Removal::AbsenceObserved
                } else {
                    Removal::Uncertain
                }
            );
            assert!(removal.begin().is_err());
        }
    }
}
