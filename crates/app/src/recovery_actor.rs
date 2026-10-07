//! Private recovery I/O. The UI never waits for a filesystem operation.
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
    fn put(&mut self, id: RecordId, pending: Pending) -> Result<(), String> {
        if self
            .items
            .get(&id)
            .is_some_and(|old| old.sequence >= pending.sequence)
        {
            return Ok(());
        }
        let old_bytes = self.items.get(&id).map_or(0, |item| item.mutation.bytes());
        let bytes = self.bytes - old_bytes + pending.mutation.bytes();
        if (self.items.len() >= MAX_PENDING && !self.items.contains_key(&id))
            || bytes > MAX_PENDING_BYTES
        {
            return Err("Recovery queue is full. Your editor text is retained; retry recovery or save/copy the draft".into());
        }
        self.items.insert(id, pending);
        self.bytes = bytes;
        Ok(())
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
    pub sequence: u64,
    pub result: Result<(), String>,
}
#[derive(Default)]
pub struct Results {
    pub acks: HashMap<RecordId, Ack>,
    pub listing: Option<Result<Listing, String>>,
    pub read: Option<(RecordId, Result<Draft, String>)>,
}
#[derive(Default)]
struct Mailbox {
    queue: Queue,
    results: Results,
    refresh: bool,
    retry: bool,
    read: Option<RecordId>,
    stopping: bool,
    flush: bool,
}
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    start: Instant,
}
pub struct Actor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl Actor {
    pub fn spawn(path: PathBuf, ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox {
                refresh: true,
                ..Default::default()
            }),
            wake: Condvar::new(),
            start: Instant::now(),
        });
        let worker = Arc::clone(&shared);
        let thread = std::thread::spawn(move || run(worker, path, ctx));
        Self {
            shared,
            thread: Some(thread),
        }
    }
    pub fn submit(&self, sequence: u64, mutation: Mutation) -> Result<RecordId, String> {
        if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
            return Err(
                "Recovery worker stopped. Retry to restart it; your current text is still editable"
                    .into(),
            );
        }
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
        mailbox.queue.put(
            id.clone(),
            Pending {
                sequence,
                mutation,
                due,
            },
        )?;
        self.shared.wake.notify_one();
        Ok(id)
    }
    pub fn cancel_writes(&self) {
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        mailbox
            .queue
            .items
            .retain(|_, item| matches!(item.mutation, Mutation::Remove { .. }));
        mailbox.queue.bytes = 0;
    }
    pub fn refresh(&self, retry: bool) {
        let mut mailbox = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        mailbox.refresh = true;
        mailbox.retry |= retry;
        self.shared.wake.notify_one();
    }
    pub fn read(&self, id: RecordId) {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .read = Some(id);
        self.shared.wake.notify_one();
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
    Refresh(bool),
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
                if mailbox.refresh {
                    mailbox.refresh = false;
                    let retry = std::mem::take(&mut mailbox.retry);
                    break Work::Refresh(retry);
                }
                if let Some(id) = mailbox.read.take() {
                    break Work::Read(id);
                }
                let flush = mailbox.stopping || mailbox.flush;
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
            Work::Refresh(retry) => {
                if retry && store.is_err() {
                    store = Store::open(&path).map_err(|error| error.to_string());
                }
                let result = store
                    .as_mut()
                    .map_err(|error| error.clone())
                    .and_then(|store| store.list().map_err(|error| error.to_string()));
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .results
                    .listing = Some(result);
            }
            Work::Read(id) => {
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
                let result = store
                    .as_mut()
                    .map_err(|error| error.clone())
                    .and_then(|store| {
                        match &item.mutation {
                            Mutation::Write(draft) => store.write(item.sequence, draft),
                            Mutation::Remove { workspace, path } => {
                                store.remove(item.sequence, workspace, path)
                            }
                        }
                        .map_err(|error| error.to_string())
                        .and_then(|outcome| match outcome {
                            MutationOutcome::Applied => Ok(()),
                            MutationOutcome::IgnoredStale => Err(
                                "Recovery rejected an outdated operation. Retry the latest draft"
                                    .into(),
                            ),
                        })
                    });
                shared
                    .mailbox
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .results
                    .acks
                    .insert(
                        id,
                        Ack {
                            sequence: item.sequence,
                            result,
                        },
                    );
            }
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft(text: &str) -> Draft {
        Draft {
            workspace: WorkspaceIdentity::Local {
                root: "/synthetic".into(),
            },
            path: "main.rs".into(),
            text: text.into(),
            base_text: "base".into(),
            base_revision: Some("r0".into()),
            modified_ms: 1,
        }
    }
    fn pending(sequence: u64, text: &str, due: u64) -> Pending {
        Pending {
            sequence,
            mutation: Mutation::Write(draft(text)),
            due: Duration::from_secs(due),
        }
    }
    #[test]
    fn fake_clock_coalesces_latest_snapshot_and_debounces() {
        let mut queue = Queue::default();
        let id = record_id(&draft("").workspace, "main.rs").unwrap();
        queue.put(id.clone(), pending(1, "first", 1)).unwrap();
        queue.put(id.clone(), pending(2, "newer", 2)).unwrap();
        queue.put(id.clone(), pending(1, "stale", 0)).unwrap();
        assert_eq!(queue.items.len(), 1);
        assert!(queue
            .take_ready(Duration::from_millis(1999), false)
            .is_none());
        let (_, item) = queue.take_ready(Duration::from_secs(2), false).unwrap();
        assert_eq!(item.sequence, 2);
        assert!(matches!(item.mutation, Mutation::Write(d) if d.text == "newer"));
        assert_eq!(queue.bytes, 0);
    }
    #[test]
    fn removal_supersedes_delayed_write_and_flush_ignores_debounce() {
        let mut queue = Queue::default();
        let draft = draft("draft");
        let id = record_id(&draft.workspace, &draft.path).unwrap();
        queue.put(id.clone(), pending(1, "stale", 100)).unwrap();
        queue
            .put(
                id,
                Pending {
                    sequence: 2,
                    mutation: Mutation::Remove {
                        workspace: draft.workspace,
                        path: draft.path,
                    },
                    due: Duration::ZERO,
                },
            )
            .unwrap();
        assert!(matches!(
            queue.take_ready(Duration::ZERO, false).unwrap().1.mutation,
            Mutation::Remove { .. }
        ));
        let id = record_id(&self::draft("").workspace, "other.rs").unwrap();
        queue.put(id, pending(3, "new draft", 100)).unwrap();
        assert!(queue.take_ready(Duration::ZERO, true).is_some());
    }
    #[test]
    fn shutdown_flushes_pending_actual_store_write() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("recovery");
        let actor = Actor::spawn(path.clone(), egui::Context::default());
        let draft = draft("pending shutdown text");
        actor.submit(1, Mutation::Write(draft.clone())).unwrap();
        drop(actor);
        let store = Store::open(&path).unwrap();
        let id = record_id(&draft.workspace, &draft.path).unwrap();
        assert_eq!(store.read(&id).unwrap().text, "pending shutdown text");
    }
}
