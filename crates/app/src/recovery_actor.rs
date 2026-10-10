//! Private recovery I/O. Admission and the close fence share one mailbox lock.
use cedar_recovery::{
    record_id, Draft, Listing, MutationOutcome, RecordId, Store, WorkspaceIdentity,
};
use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub const DEBOUNCE: Duration = Duration::from_secs(1);
const MAX_PENDING: usize = 64;
const MAX_PENDING_BYTES: usize = 128 * 1024 * 1024;
const MAX_UNOBSERVED: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Availability {
    #[default]
    Starting,
    Ready,
    Unavailable(String),
    Stopped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Write,
    Remove,
}
/// An I/O error may follow rename/unlink. It cannot prove presence or absence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    NotInvoked(String),
    Applied,
    PossiblyApplied(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub generation: u64,
    pub serial: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settlement {
    Quiescent(Ticket),
    Resumed(Ticket),
}
pub enum Mutation {
    Write(Draft),
    Remove {
        workspace: WorkspaceIdentity,
        path: String,
    },
}
impl Mutation {
    fn bytes(&self) -> usize {
        match self {
            Self::Write(draft) => draft.text.len() + draft.base_text.len(),
            Self::Remove { .. } => 0,
        }
    }
    fn kind(&self) -> OperationKind {
        match self {
            Self::Write(_) => OperationKind::Write,
            Self::Remove { .. } => OperationKind::Remove,
        }
    }
}
pub struct Pending {
    pub sequence: u64,
    pub mutation: Mutation,
    due: Duration,
}
#[derive(Default)]
struct Queue {
    items: HashMap<RecordId, Pending>,
    bytes: usize,
}
impl Queue {
    fn put(&mut self, id: RecordId, pending: Pending) -> Result<Option<Pending>, String> {
        if self
            .items
            .get(&id)
            .is_some_and(|old| old.sequence >= pending.sequence)
        {
            return Err("Recovery rejected an outdated submission".into());
        }
        let old_bytes = self.items.get(&id).map_or(0, |item| item.mutation.bytes());
        let bytes = self.bytes - old_bytes + pending.mutation.bytes();
        if (self.items.len() >= MAX_PENDING && !self.items.contains_key(&id))
            || bytes > MAX_PENDING_BYTES
        {
            return Err("Recovery queue is full. Your editor text is retained; retry recovery or save/copy the draft".into());
        }
        let previous = self.items.insert(id, pending);
        self.bytes = bytes;
        Ok(previous)
    }
    fn take_ready(&mut self, now: Duration, flush: bool) -> Option<(RecordId, Pending)> {
        let id = self
            .items
            .iter()
            .filter(|(_, item)| flush || item.due <= now)
            .min_by_key(|(_, item)| item.sequence)
            .map(|(id, _)| id.clone())?;
        let pending = self.items.remove(&id)?;
        self.bytes -= pending.mutation.bytes();
        Some((id, pending))
    }
    fn delay(&self, now: Duration) -> Duration {
        self.items
            .values()
            .map(|item| item.due.saturating_sub(now))
            .min()
            .unwrap_or(Duration::from_secs(60))
    }
}
pub struct Ack {
    pub generation: u64,
    pub sequence: u64,
    pub kind: OperationKind,
    pub effect: Effect,
}
#[derive(Default)]
pub struct Results {
    // Keep every effect, including an older write followed by a rejected intent.
    pub acks: Vec<(RecordId, Ack)>,
    pub listing: Option<(u64, Result<Listing, String>)>,
    pub read: Option<(RecordId, Result<Draft, String>)>,
    pub settlement: Option<Settlement>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fence {
    #[default]
    Running,
    Draining(Ticket),
    Paused(Ticket),
}
#[derive(Default)]
struct Mailbox {
    queue: Queue,
    results: Results,
    availability: Availability,
    fence: Fence,
    resume: bool,
    refresh: bool,
    listing_epoch: u64,
    retry: bool,
    read: Option<RecordId>,
    stopping: bool,
    flush: bool,
}
impl Mailbox {
    fn acknowledge(&mut self, generation: u64, id: RecordId, item: Pending, effect: Effect) {
        self.results.acks.push((
            id,
            Ack {
                generation,
                sequence: item.sequence,
                kind: item.mutation.kind(),
                effect,
            },
        ));
    }
    fn cancel(&mut self, generation: u64, kind: OperationKind, message: &str) {
        let ids: Vec<_> = self
            .queue
            .items
            .iter()
            .filter(|(_, item)| item.mutation.kind() == kind)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let item = self.queue.items.remove(&id).unwrap();
            self.queue.bytes -= item.mutation.bytes();
            self.acknowledge(generation, id, item, Effect::NotInvoked(message.into()));
        }
    }
    fn freeze(&mut self, ticket: Ticket) -> Result<(), String> {
        if self.fence != Fence::Running {
            return Err("Recovery is still settling an earlier close request. Keep editing and wait for it to resume".into());
        }
        self.fence = Fence::Draining(ticket);
        self.refresh = false;
        self.retry = false;
        self.read = None;
        self.cancel(
            ticket.generation,
            OperationKind::Remove,
            "Removal was not started; remaining recovery copies were retained",
        );
        self.flush = true;
        Ok(())
    }
    // Called by the sole worker only between operations, never by the UI.
    fn settle(&mut self) -> bool {
        if let Fence::Draining(ticket) = self.fence {
            if !self.queue.items.is_empty() {
                return false;
            }
            self.fence = Fence::Paused(ticket);
            if !self.resume {
                self.results.settlement = Some(Settlement::Quiescent(ticket));
            }
        }
        if let Fence::Paused(ticket) = self.fence {
            if self.resume {
                self.resume = false;
                self.fence = Fence::Running;
                self.results.settlement = Some(Settlement::Resumed(ticket));
            }
        }
        true
    }
}
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    start: Instant,
    generation: u64,
    #[cfg(test)]
    before_mutation: Mutex<Option<TestPause>>,
    #[cfg(test)]
    before_read: Mutex<Option<TestPause>>,
}
#[cfg(test)]
struct TestPause {
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
    panic_after: bool,
}
pub struct Actor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl Actor {
    pub fn spawn(path: PathBuf, ctx: egui::Context, generation: u64) -> Self {
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox {
                refresh: true,
                listing_epoch: 1,
                ..Default::default()
            }),
            wake: Condvar::new(),
            start: Instant::now(),
            generation,
            #[cfg(test)]
            before_mutation: Mutex::new(None),
            #[cfg(test)]
            before_read: Mutex::new(None),
        });
        let worker = Arc::clone(&shared);
        let thread = std::thread::spawn(move || run(worker, path, ctx));
        Self {
            shared,
            thread: Some(thread),
        }
    }
    pub fn availability(&self) -> Availability {
        if self.finished() {
            return Availability::Stopped;
        }
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .availability
            .clone()
    }
    /// Every submission error is NotInvoked, even when a prior operation is in flight.
    pub fn submit(&self, sequence: u64, mutation: Mutation) -> Result<RecordId, String> {
        let (workspace, path) = match &mutation {
            Mutation::Write(draft) => (&draft.workspace, draft.path.as_str()),
            Mutation::Remove { workspace, path } => (workspace, path.as_str()),
        };
        let id = record_id(workspace, path).map_err(|error| error.to_string())?;
        let due = self.shared.start.elapsed()
            + if matches!(mutation, Mutation::Write(_)) {
                DEBOUNCE
            } else {
                Duration::ZERO
            };
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.finished() || mailbox.stopping {
            return Err(
                "Recovery worker stopped. Retry recovery; current text remains editable".into(),
            );
        }
        if mailbox.fence != Fence::Running {
            return Err(
                "Recovery is paused while a close request settles. Current text remains editable"
                    .into(),
            );
        }
        match &mailbox.availability {
            Availability::Ready => {}
            Availability::Unavailable(error) => return Err(error.clone()),
            Availability::Starting => {
                return Err("Recovery is starting. Current text remains editable".into())
            }
            Availability::Stopped => return Err("Recovery worker stopped. Retry recovery".into()),
        }
        // Reserve room for every queued completion and the one in-flight operation.
        if mailbox.results.acks.len() + mailbox.queue.items.len() + 1 >= MAX_UNOBSERVED {
            return Err("Recovery results are waiting to be observed. Retry recovery".into());
        }
        if let Some(previous) = mailbox.queue.put(
            id.clone(),
            Pending {
                sequence,
                mutation,
                due,
            },
        )? {
            mailbox.acknowledge(
                self.shared.generation,
                id.clone(),
                previous,
                Effect::NotInvoked("Replaced before recovery I/O by a newer submission".into()),
            );
        }
        self.shared.wake.notify_one();
        Ok(id)
    }
    pub fn quiesce(&self, ticket: Ticket) -> Result<(), String> {
        if ticket.generation != self.shared.generation || self.finished() {
            return Err("Recovery worker stopped without a quiescence proof. Keep editing and retry recovery".into());
        }
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .freeze(ticket)?;
        self.shared.wake.notify_one();
        Ok(())
    }
    pub fn resume(&self, ticket: Ticket) {
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if matches!(mailbox.fence, Fence::Draining(current) | Fence::Paused(current) if current == ticket)
        {
            mailbox.resume = true;
            self.shared.wake.notify_one();
        }
    }
    pub fn cancel_writes(&self) {
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // A close fence always drains the writes it already accepted.
        if mailbox.fence == Fence::Running {
            mailbox.cancel(
                self.shared.generation,
                OperationKind::Write,
                "Recovery was turned off before this write started",
            );
        }
    }
    pub fn refresh(&self, retry: bool) -> Option<u64> {
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if mailbox.fence == Fence::Running {
            mailbox.refresh = true;
            mailbox.listing_epoch += 1;
            mailbox.retry |= retry;
            if retry {
                mailbox.availability = Availability::Starting;
            }
            self.shared.wake.notify_one();
            return Some(mailbox.listing_epoch);
        }
        None
    }
    pub fn read(&self, id: RecordId) {
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if mailbox.fence == Fence::Running {
            mailbox.read = Some(id);
            self.shared.wake.notify_one();
        }
    }
    pub fn poll(&self) -> Results {
        std::mem::take(
            &mut self
                .shared
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .results,
        )
    }
    pub fn flush(&self) {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .flush = true;
        self.shared.wake.notify_one();
    }
    #[cfg(test)]
    pub fn hold_next_operation(
        &self,
        panic_after: bool,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ) {
        let (entered, observed) = std::sync::mpsc::sync_channel(1);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        *self.shared.before_mutation.lock().unwrap() = Some(TestPause {
            entered,
            release: wait,
            panic_after,
        });
        (observed, release)
    }
    #[cfg(test)]
    pub fn hold_next_read(
        &self,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ) {
        let (entered, observed) = std::sync::mpsc::sync_channel(1);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        *self.shared.before_read.lock().unwrap() = Some(TestPause {
            entered,
            release: wait,
            panic_after: false,
        });
        (observed, release)
    }
    #[cfg(test)]
    pub fn queued(&self) -> usize {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .queue
            .items
            .len()
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stopping = true;
        self.shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
enum Work {
    Refresh { retry: bool, epoch: u64 },
    Read(RecordId),
    Mutate(RecordId, Box<Pending>),
}
fn run(shared: Arc<Shared>, path: PathBuf, ctx: egui::Context) {
    let mut store = Store::open(&path).map_err(|error| error.to_string());
    loop {
        let work = {
            let mut mailbox = shared
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            loop {
                mailbox.settle();
                if mailbox.results.settlement.is_some() {
                    ctx.request_repaint();
                }
                if matches!(mailbox.fence, Fence::Paused(_)) {
                    // Once proof is published, even Drop cannot run another operation.
                    if mailbox.stopping {
                        return;
                    }
                    mailbox = shared
                        .wake
                        .wait(mailbox)
                        .unwrap_or_else(|error| error.into_inner());
                    continue;
                }
                if mailbox.fence == Fence::Running {
                    if mailbox.refresh {
                        mailbox.refresh = false;
                        let retry = std::mem::take(&mut mailbox.retry);
                        break Work::Refresh {
                            retry,
                            epoch: mailbox.listing_epoch,
                        };
                    }
                    if let Some(id) = mailbox.read.take() {
                        break Work::Read(id);
                    }
                }
                let flush = mailbox.stopping
                    || mailbox.flush
                    || matches!(mailbox.fence, Fence::Draining(_));
                if let Some((id, item)) = mailbox.queue.take_ready(shared.start.elapsed(), flush) {
                    break Work::Mutate(id, Box::new(item));
                }
                mailbox.flush = false;
                if mailbox.stopping {
                    return;
                }
                let delay = mailbox.queue.delay(shared.start.elapsed());
                mailbox = shared
                    .wake
                    .wait_timeout(mailbox, delay)
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
            }
        };
        match work {
            Work::Refresh { retry, epoch } => {
                if retry && store.is_err() {
                    store = Store::open(&path).map_err(|error| error.to_string());
                }
                let result = store
                    .as_mut()
                    .map_err(|error| error.clone())
                    .and_then(|store| store.list().map_err(|error| error.to_string()));
                let mut mailbox = shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if mailbox.listing_epoch == epoch {
                    mailbox.availability = match &result {
                        Ok(_) if retry || mailbox.availability == Availability::Starting => {
                            Availability::Ready
                        }
                        Ok(_) => mailbox.availability.clone(),
                        Err(error) => Availability::Unavailable(error.clone()),
                    };
                }
                mailbox.results.listing = Some((epoch, result));
            }
            Work::Read(id) => {
                #[cfg(test)]
                {
                    let pause = shared.before_read.lock().unwrap().take();
                    if let Some(pause) = pause {
                        let _ = pause.entered.send(());
                        let _ = pause.release.recv();
                    }
                }
                let result = store
                    .as_mut()
                    .map_err(|error| error.clone())
                    .and_then(|store| store.read(&id).map_err(|error| error.to_string()));
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .results
                    .read = Some((id, result));
            }
            Work::Mutate(id, item) => {
                #[cfg(test)]
                {
                    let pause = shared.before_mutation.lock().unwrap().take();
                    if let Some(pause) = pause {
                        let _ = pause.entered.send(());
                        let _ = pause.release.recv();
                        assert!(
                            !pause.panic_after,
                            "synthetic recovery worker failure before acknowledgement"
                        );
                    }
                }
                let effect = match store.as_mut() {
                    Err(error) => Effect::NotInvoked(error.clone()),
                    Ok(store) => match match &item.mutation {
                        Mutation::Write(draft) => store.write(item.sequence, draft),
                        Mutation::Remove { workspace, path } => {
                            store.remove(item.sequence, workspace, path)
                        }
                    } {
                        Ok(MutationOutcome::Applied) => Effect::Applied,
                        Ok(MutationOutcome::IgnoredStale) => Effect::NotInvoked(
                            "Recovery rejected an outdated operation. Retry the latest draft"
                                .into(),
                        ),
                        Err(error) => Effect::PossiblyApplied(error.to_string()),
                    },
                };
                let mut mailbox = shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if let Effect::PossiblyApplied(error) = &effect {
                    mailbox.availability = Availability::Unavailable(error.clone());
                }
                mailbox.acknowledge(shared.generation, id, *item, effect);
            }
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
#[path = "recovery_actor_tests.rs"]
mod tests;
