//! NONSHIPPING admission model. No filesystem operations and no Store integration.
//! Observation is evidence, not authority. Admission owns the retained handle.
use super::policy::{self, Identity, ObjectFacts, ParseError, Rejection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Roles are test-model assignments, not proof of CREATE_NEW provenance.
/// In particular OPEN_EXISTING can never establish ownership of a temporary.
pub enum Role {
    Ancestor,
    Root,
    RecordDirectory,
    Lock,
    Temporary,
    ExistingRecord,
}
impl Role {
    pub fn directory(self) -> bool {
        matches!(self, Self::Ancestor | Self::Root | Self::RecordDirectory)
    }
    pub fn requires_empty(self) -> bool {
        matches!(self, Self::Lock | Self::Temporary)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Query,
    Bounds,
    Inheritable,
    IdentityChanged,
    TokenChanged,
    Policy(Rejection),
    Parse(ParseError),
    WrongOperation,
    MutationUncertain,
}

pub struct Observation {
    pub identity: Identity,
    pub facts: ObjectFacts,
    pub descriptor: Vec<u8>,
    pub user: Vec<u8>,
}
pub trait Queries<H> {
    fn observe(&mut self, handle: &H) -> Result<Observation, Error>;
}

fn assess(role: Role, observation: &Observation) -> Result<(), Error> {
    if role.requires_empty() && !observation.facts.empty {
        return Err(Error::Policy(Rejection::NonemptyFile));
    }
    // The older empty-object probe's parser also enforces emptiness. The model
    // explicitly owns that role-specific check above; existing record contents
    // are allowed, while all other structural and descriptor checks are reused.
    let facts = ObjectFacts {
        empty: true,
        ..observation.facts
    };
    let verdict = policy::assess(
        &observation.descriptor,
        &observation.user,
        facts,
        role.directory(),
    )
    .map_err(Error::Parse)?;
    verdict
        .rejection
        .map_or(Ok(()), |reason| Err(Error::Policy(reason)))
}

pub struct Admitted<H> {
    handle: H,
    role: Role,
    identity: Identity,
    user: Vec<u8>,
}
impl<H> Admitted<H> {
    pub fn admit(handle: H, role: Role, queries: &mut impl Queries<H>) -> Result<Self, Error> {
        let first = queries.observe(&handle)?;
        assess(role, &first)?;
        let admitted = Self {
            handle,
            role,
            identity: first.identity,
            user: first.user,
        };
        admitted.revalidate(queries)?;
        Ok(admitted)
    }
    pub fn revalidate(&self, queries: &mut impl Queries<H>) -> Result<(), Error> {
        let current = queries.observe(&self.handle)?;
        if current.identity != self.identity {
            return Err(Error::IdentityChanged);
        }
        if current.user != self.user {
            return Err(Error::TokenChanged);
        }
        assess(self.role, &current)
    }
    /// Only a fake sink is provided by this test harness. A successful check is
    /// not a race-free production read/write/delete capability.
    pub fn exercise(
        self,
        operation: Operation,
        queries: &mut impl Queries<H>,
        sink: &mut impl ModelSink<H>,
    ) -> Result<Attempted, Error> {
        let allowed = matches!(
            (self.role, operation),
            (Role::Temporary, Operation::Payload)
                | (Role::ExistingRecord, Operation::Read | Operation::Remove)
        );
        if !allowed {
            return Err(Error::WrongOperation);
        }
        self.revalidate(queries)?;
        sink.call(&self.handle, operation);
        Ok(Attempted::OutcomeUnknown)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attempted {
    OutcomeUnknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Payload,
    Read,
    Remove,
}
pub trait ModelSink<H> {
    fn call(&mut self, handle: &H, operation: Operation);
}

/// No target-ID conditional rename is assumed. A handle check followed by a
/// pathname mutation cannot prove which target that mutation affected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    NotAttempted,
    Uncertain,
    PublishedObserved,
    DurabilityAcknowledged,
}
impl Mutation {
    pub fn begin(&mut self) -> Result<(), Error> {
        if *self != Self::NotAttempted {
            return Err(Error::MutationUncertain);
        }
        *self = Self::Uncertain;
        Ok(())
    }
    pub fn observe_publication(&mut self, source: Identity, reopened: Result<Identity, Error>) {
        if *self == Self::Uncertain && reopened.is_ok_and(|id| id == source) {
            *self = Self::PublishedObserved;
        }
    }
    pub fn acknowledge_durability(&mut self, acknowledged: bool) {
        if *self == Self::PublishedObserved && acknowledged {
            *self = Self::DurabilityAcknowledged;
        }
    }
}

/// A fake remove sink being called is never proof of removal or durability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    NotAttempted,
    Uncertain,
    AbsenceObserved,
}
impl Removal {
    pub fn begin(&mut self) -> Result<(), Error> {
        if *self != Self::NotAttempted {
            return Err(Error::MutationUncertain);
        }
        *self = Self::Uncertain;
        Ok(())
    }
    pub fn observe_absence(&mut self, absent: Result<bool, Error>) {
        if *self == Self::Uncertain && absent == Ok(true) {
            *self = Self::AbsenceObserved;
        }
    }
}
