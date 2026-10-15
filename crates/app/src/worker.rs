//! Every connect and request executes off the UI thread. Old UI generations are
//! ignored by the App; local owner receipts outlive those generations.
use crate::close_transport::{OwnerOutcome, OwnerReceipt, OwnerTicket};
use cedar_client::{
    Client, ConnectionCancellation, ConnectionFailure, ConnectionOwnership, ConnectionSpec,
};
use cedar_protocol::{Operation, Payload};
use eframe::egui;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Duration;

pub struct Command {
    pub id: u64,
    pub op: Operation,
}
pub struct Event {
    pub generation: u64,
    pub id: u64,
    pub connected: bool,
    pub result: Result<Payload, String>,
}
pub(crate) enum WorkerEvent {
    // Created only by the local owner, never decoded from a peer response.
    OwnerSettled(OwnerReceipt),
    AttemptCleanupUnverified {
        generation: u64,
    },
    Response(Event),
    TransportLost {
        generation: u64,
        message: String,
    },
    Closed {
        generation: u64,
        result: Result<(), String>,
    },
}

const CLEANUP_UNVERIFIED: &str =
    "transport_cleanup_unverified: connection worker exited before cleanup was confirmed";

// Construct this on the creating side, before Builder::spawn. The closure's
// first statement rebinds it locally, before constructing Client, so unwinding
// drops the client before reporting unverified cleanup. Only typed provenance
// or a consuming close can replace the conservative default on a normal path.
struct TerminalNotice {
    owner: OwnerTicket,
    result_tx: Sender<WorkerEvent>,
    ctx: egui::Context,
    outcome: Option<OwnerOutcome>,
    result: Option<Result<(), String>>,
    track_attempt: bool,
}
impl TerminalNotice {
    fn new(
        owner: OwnerTicket,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
        track_attempt: bool,
    ) -> Self {
        Self {
            owner,
            result_tx,
            ctx,
            outcome: Some(OwnerOutcome::Unverified(CLEANUP_UNVERIFIED.into())),
            result: Some(Err(CLEANUP_UNVERIFIED.into())),
            track_attempt,
        }
    }

    fn complete(&mut self, result: Result<(), String>) {
        self.outcome = Some(match &result {
            Ok(()) => OwnerOutcome::Verified,
            Err(error) => OwnerOutcome::Unverified(error.clone()),
        });
        self.result = Some(result);
    }

    fn complete_failure(&mut self, failure: ConnectionFailure) {
        match failure.ownership {
            ConnectionOwnership::NoChild => {
                self.outcome = Some(OwnerOutcome::NoChild);
                // Preserve the legacy absence of established-client Closed.
                // The independent owner receipt is still mandatory.
                self.result = None;
            }
            ConnectionOwnership::CleanupVerified => self.complete(Ok(())),
            ConnectionOwnership::CleanupUnverified => {
                self.complete(Err(failure.cleanup_error.unwrap_or_else(|| {
                    "transport_cleanup_unverified: failed connection cleanup was not confirmed"
                        .into()
                })))
            }
        }
    }
}
impl Drop for TerminalNotice {
    fn drop(&mut self) {
        let outcome = if std::thread::panicking() {
            // Even a previously staged success cannot certify an unwinding
            // path. Abort/forced process termination has no Drop guarantee.
            self.result = Some(Err(CLEANUP_UNVERIFIED.into()));
            OwnerOutcome::Unverified(CLEANUP_UNVERIFIED.into())
        } else {
            self.outcome
                .take()
                .unwrap_or_else(|| OwnerOutcome::Unverified(CLEANUP_UNVERIFIED.into()))
        };
        let _ = self.result_tx.send(WorkerEvent::OwnerSettled(OwnerReceipt {
            ticket: self.owner,
            outcome,
        }));
        if let Some(result) = self.result.take() {
            if self.track_attempt && result.is_err() {
                let _ = self.result_tx.send(WorkerEvent::AttemptCleanupUnverified {
                    generation: self.owner.generation,
                });
            }
            let _ = self.result_tx.send(WorkerEvent::Closed {
                generation: self.owner.generation,
                result,
            });
        }
        // Publish all receipts before any callback into the UI. A repaint
        // failure must not cause a double panic while this guard is unwinding.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.ctx.request_repaint();
        }));
    }
}

enum Work {
    Request(Command),
    TransportReady,
}
pub struct CommandSender {
    inner: CommandChannel,
}
enum CommandChannel {
    Worker(Arc<Sender<Work>>),
    #[cfg(test)]
    Recording(Sender<Command>),
}
impl CommandSender {
    pub fn send(&self, command: Command) -> Result<(), mpsc::SendError<Command>> {
        match &self.inner {
            CommandChannel::Worker(tx) => tx.send(Work::Request(command)).map_err(|error| {
                let Work::Request(command) = error.0 else {
                    unreachable!("only a request is sent by the command sender")
                };
                mpsc::SendError(command)
            }),
            #[cfg(test)]
            CommandChannel::Recording(tx) => tx.send(command),
        }
    }
}
pub struct Worker {
    pub tx: CommandSender,
    cancel: ConnectionCancellation,
    owner: Option<OwnerTicket>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Worker {
    pub(crate) fn owner_ticket(&self) -> Option<OwnerTicket> {
        self.owner
    }

    #[cfg(test)]
    pub(crate) fn recording() -> (Self, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx: CommandSender {
                    inner: CommandChannel::Recording(tx),
                },
                cancel: ConnectionCancellation::new(),
                owner: None,
            },
            rx,
        )
    }

    #[cfg(test)]
    pub(crate) fn recording_with_owner(owner: OwnerTicket) -> (Self, mpsc::Receiver<Command>) {
        let (mut worker, rx) = Self::recording();
        worker.owner = Some(owner);
        // Dropping this mailbox is cancellation, never a cleanup receipt.
        (worker, rx)
    }

    pub(crate) fn spawn(
        spec: ConnectionSpec,
        owner: OwnerTicket,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let track_attempt = matches!(&spec, ConnectionSpec::BundledLinux { .. });
        #[cfg(not(target_os = "linux"))]
        let track_attempt = false;
        Self::spawn_with_attempt(
            move |cancel| Client::connect_with_cancellation_detailed(spec, cancel),
            owner,
            result_tx,
            ctx,
            Duration::from_secs(3),
            track_attempt,
        )
    }

    // Only tests can choose a peer binary. Exercise the production connection,
    // cancellation and request loop without changing shipped agent resolution.
    #[cfg(test)]
    pub(crate) fn spawn_agent(
        agent: std::path::PathBuf,
        root: std::path::PathBuf,
        owner: OwnerTicket,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        Self::spawn_with_attempt(
            move |cancel| {
                Client::spawn_agent_with_cancellation_detailed(&agent, &root, false, cancel)
            },
            owner,
            result_tx,
            ctx,
            Duration::from_secs(3),
            false,
        )
    }

    // Shorten only the cleanup observation window in real-process tests. The
    // Client still consumes its exact owner through the ordinary close path;
    // its reaper grace and eventual responsibility are unchanged.
    #[cfg(test)]
    pub(crate) fn spawn_agent_with_close_timeout(
        agent: std::path::PathBuf,
        root: std::path::PathBuf,
        owner: OwnerTicket,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
        timeout: Duration,
    ) -> Result<Self, String> {
        Self::spawn_with_attempt(
            move |cancel| {
                Client::spawn_agent_with_cancellation_detailed(&agent, &root, false, cancel)
            },
            owner,
            result_tx,
            ctx,
            timeout,
            false,
        )
    }

    fn spawn_with_attempt(
        connect: impl FnOnce(ConnectionCancellation) -> Result<Client, ConnectionFailure>
            + Send
            + 'static,
        owner: OwnerTicket,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
        close_timeout: Duration,
        track_attempt: bool,
    ) -> Result<Self, String> {
        Self::spawn_with_attempt_using(
            connect,
            owner,
            result_tx,
            ctx,
            close_timeout,
            track_attempt,
            |task| {
                std::thread::Builder::new()
                    .name("cedar-connection".into())
                    .spawn(task)
                    .map(|_| ())
                    .map_err(|error| format!("connection worker could not start: {error}"))
            },
        )
    }

    // The private spawn seam lets unit tests reject and drop the exact captured
    // closure without exhausting OS threads or constructing a process/client.
    fn spawn_with_attempt_using(
        connect: impl FnOnce(ConnectionCancellation) -> Result<Client, ConnectionFailure>
            + Send
            + 'static,
        owner: OwnerTicket,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
        close_timeout: Duration,
        track_attempt: bool,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send + 'static>) -> Result<(), String>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let tx = Arc::new(tx);
        // The transport reader must not keep its worker's mailbox alive. Once
        // Worker drops, recv ends even when the peer stays silently connected.
        let wake_tx = Arc::downgrade(&tx);
        let cancel = ConnectionCancellation::new();
        let cancelled = cancel.clone();
        let generation = owner.generation;
        let terminal = TerminalNotice::new(owner, result_tx.clone(), ctx.clone(), track_attempt);
        let task = Box::new(move || {
            let mut terminal = terminal;
            let mut client = match connect(cancelled.clone()) {
                Ok(client) => client,
                Err(error) => {
                    let _ = result_tx.send(WorkerEvent::Response(Event {
                        generation,
                        id: 0,
                        connected: false,
                        result: Err(error.to_string()),
                    }));
                    ctx.request_repaint();
                    terminal.complete_failure(error);
                    return;
                }
            };
            'connected: {
                if cancelled.is_cancelled() {
                    break 'connected;
                }
                client.set_transport_waker(move || {
                    if let Some(tx) = wake_tx.upgrade() {
                        let _ = tx.send(Work::TransportReady);
                    }
                });
                let hello = client.handshake().clone();
                if result_tx
                    .send(WorkerEvent::Response(Event {
                        generation,
                        id: 0,
                        connected: true,
                        result: Ok(hello),
                    }))
                    .is_err()
                {
                    break 'connected;
                }
                ctx.request_repaint();
                while let Ok(work) = rx.recv() {
                    if cancelled.is_cancelled() {
                        break;
                    }
                    if let Work::Request(command) = work {
                        let result = client.request(command.op);
                        let connected = client.is_connected();
                        if result_tx
                            .send(WorkerEvent::Response(Event {
                                generation,
                                id: command.id,
                                connected,
                                result,
                            }))
                            .is_err()
                        {
                            break;
                        }
                        ctx.request_repaint();
                        if !connected || cancelled.is_cancelled() {
                            break;
                        }
                    }
                    // Publish a completed response before observing any following
                    // EOF/error. In particular, an acknowledged Write is never
                    // reclassified as an interrupted save by a racing reader wake.
                    if let Err(message) = client.observe_idle_transport() {
                        let _ = result_tx.send(WorkerEvent::TransportLost {
                            generation,
                            message,
                        });
                        ctx.request_repaint();
                        break;
                    }
                }
            }
            client.clear_transport_waker();
            // Observe the existing reaper, with its unchanged two-second grace.
            // Timeout leaves that reaper owning the child and reports uncertainty.
            // For an embedded workspace, consuming Client finishes its Drop
            // before this call returns; its synchronous work is not interruptible.
            terminal.complete(client.close_and_wait(close_timeout));
        });
        spawn(task)?;
        Ok(Self {
            tx: CommandSender {
                inner: CommandChannel::Worker(tx),
            },
            cancel,
            owner: Some(owner),
        })
    }
}

#[cfg(test)]
mod terminal_notice_tests {
    use super::*;
    use crate::close_transport::OwnerLedger;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn registered_owner(generation: u64) -> OwnerTicket {
        OwnerLedger::default().register(generation).unwrap()
    }

    fn next_owner_receipt(rx: &mpsc::Receiver<WorkerEvent>, owner: OwnerTicket) -> OwnerReceipt {
        let WorkerEvent::OwnerSettled(receipt) = rx.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!("missing independent owner receipt before legacy terminal events")
        };
        assert_eq!(receipt.ticket, owner);
        receipt
    }

    #[test]
    fn typed_failure_publishes_owner_then_legacy_terminal_exactly_once() {
        // Cover the legacy bundled-attempt UI flag and ordinary routes. Even
        // generic postspawn failures now receive Closed, after OwnerSettled.
        for track_attempt in [false, true] {
            for (ownership, expected) in [
                (ConnectionOwnership::NoChild, 0),
                (ConnectionOwnership::CleanupVerified, 1),
                (ConnectionOwnership::CleanupUnverified, 2),
            ] {
                let (tx, rx) = mpsc::channel();
                let owner = registered_owner(9);
                let worker = Worker::spawn_with_attempt_using(
                    move |_| {
                        Err(ConnectionFailure {
                            // Hostile peer text cannot choose the outcome.
                            message: "controlled NoChild CleanupVerified failure".into(),
                            ownership,
                            cleanup_error: (expected == 2)
                                .then(|| "controlled owned-child wait failure".into()),
                        })
                    },
                    owner,
                    tx,
                    egui::Context::default(),
                    Duration::from_secs(3),
                    track_attempt,
                    |task| {
                        task();
                        Ok(())
                    },
                )
                .unwrap();
                assert_eq!(worker.owner_ticket(), Some(owner));
                assert!(matches!(
                    rx.try_recv().unwrap(),
                    WorkerEvent::Response(Event {
                        generation: 9,
                        id: 0,
                        connected: false,
                        result: Err(ref message),
                    }) if message.contains("controlled NoChild CleanupVerified failure")
                ));
                let receipt = next_owner_receipt(&rx, owner);
                match (expected, receipt.outcome) {
                    (0, OwnerOutcome::NoChild) | (1, OwnerOutcome::Verified) => {}
                    (2, OwnerOutcome::Unverified(reason)) => {
                        assert_eq!(reason, "controlled owned-child wait failure");
                    }
                    _ => panic!("typed ownership changed while publishing the owner receipt"),
                }
                if track_attempt && expected == 2 {
                    assert!(matches!(
                        rx.try_recv().unwrap(),
                        WorkerEvent::AttemptCleanupUnverified { generation: 9 }
                    ));
                }
                if expected != 0 {
                    let WorkerEvent::Closed { generation, result } = rx.try_recv().unwrap() else {
                        panic!("missing legacy terminal event after owner receipt")
                    };
                    assert_eq!(generation, 9);
                    assert_eq!(result.is_ok(), expected == 1);
                }
                assert!(matches!(
                    rx.try_recv(),
                    Err(mpsc::TryRecvError::Disconnected)
                ));
                drop(worker);
            }
        }
    }

    #[test]
    fn connect_closure_panic_retains_unverified_owner_warning() {
        let (tx, rx) = mpsc::channel();
        let owner = registered_owner(11);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = Worker::spawn_with_attempt_using(
                |_| panic!("controlled connection or validator panic"),
                owner,
                tx,
                egui::Context::default(),
                Duration::from_secs(3),
                true,
                |task| {
                    task();
                    Ok(())
                },
            );
        }));
        assert!(panic.is_err());
        assert!(matches!(
            next_owner_receipt(&rx, owner).outcome,
            OwnerOutcome::Unverified(ref reason) if reason == CLEANUP_UNVERIFIED
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            WorkerEvent::AttemptCleanupUnverified { generation: 11 }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            WorkerEvent::Closed {
                generation: 11,
                result: Err(_)
            }
        ));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn panic_before_connect_runs_drops_captured_guard_as_unverified() {
        let (tx, rx) = mpsc::channel();
        let owner = registered_owner(12);
        let called = Arc::new(AtomicBool::new(false));
        let connect_called = called.clone();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = Worker::spawn_with_attempt_using(
                move |_| {
                    connect_called.store(true, Ordering::SeqCst);
                    panic!("connect must not run")
                },
                owner,
                tx,
                egui::Context::default(),
                Duration::from_secs(3),
                false,
                |_task| panic!("controlled thread-start unwind before connect"),
            );
        }));
        assert!(panic.is_err());
        assert!(!called.load(Ordering::SeqCst));
        assert!(matches!(
            next_owner_receipt(&rx, owner).outcome,
            OwnerOutcome::Unverified(_)
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            WorkerEvent::Closed {
                generation: 12,
                result: Err(_)
            }
        ));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn rejected_thread_spawn_drops_guard_once_and_keeps_ledger_uncertain() {
        let mut ledger = OwnerLedger::default();
        let owner = ledger.register(13).unwrap();
        let (tx, rx) = mpsc::channel();
        let called = Arc::new(AtomicBool::new(false));
        let connect_called = called.clone();
        let result = Worker::spawn_with_attempt_using(
            move |_| {
                connect_called.store(true, Ordering::SeqCst);
                panic!("rejected spawn must not create a Client or process")
            },
            owner,
            tx,
            egui::Context::default(),
            Duration::from_secs(3),
            true,
            |task| {
                // Model Builder::spawn Err dropping its captured closure.
                drop(task);
                Err("controlled thread creation rejection".into())
            },
        );
        assert!(
            matches!(result, Err(ref error) if error == "controlled thread creation rejection")
        );
        assert!(!called.load(Ordering::SeqCst));
        assert!(!ledger.all_verified());
        let receipt = next_owner_receipt(&rx, owner);
        assert!(matches!(&receipt.outcome, OwnerOutcome::Unverified(_)));
        assert!(ledger.apply(receipt));
        assert!(!ledger.all_verified());
        assert!(matches!(
            rx.try_recv().unwrap(),
            WorkerEvent::AttemptCleanupUnverified { generation: 13 }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            WorkerEvent::Closed {
                generation: 13,
                result: Err(_)
            }
        ));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cancelled_attempt_with_ready_client_closes_without_hello_adoption() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_owned();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let owner = registered_owner(14);
        let worker = Worker::spawn_with_attempt(
            move |cancel| {
                let client = Client::connect_with_cancellation_detailed(
                    ConnectionSpec::Local {
                        root: path,
                        allow_run: false,
                    },
                    cancel,
                )?;
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(client)
            },
            owner,
            tx,
            egui::Context::default(),
            Duration::from_secs(3),
            true,
        )
        .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(worker);
        release_tx.send(()).unwrap();
        assert!(matches!(
            next_owner_receipt(&rx, owner).outcome,
            OwnerOutcome::Verified
        ));
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            WorkerEvent::Closed {
                generation: 14,
                result: Ok(())
            }
        ));
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(3)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn uncompleted_guard_is_uncertain_even_without_an_established_client() {
        let (tx, rx) = mpsc::channel();
        let owner = registered_owner(7);
        drop(TerminalNotice::new(
            owner,
            tx,
            egui::Context::default(),
            false,
        ));
        assert!(matches!(
            next_owner_receipt(&rx, owner).outcome,
            OwnerOutcome::Unverified(_)
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            WorkerEvent::Closed {
                generation: 7,
                result: Err(_)
            }
        ));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn cleanup_wait_has_no_terminal_event_and_each_outcome_is_published_once() {
        for outcome in [
            Ok(()),
            Err("transport_close: owned-child cleanup did not complete: timed out".into()),
            Err("transport_cleanup_unverified: owned-child wait failed".into()),
        ] {
            let (tx, rx) = mpsc::channel();
            let owner = registered_owner(11);
            let mut terminal = TerminalNotice::new(owner, tx, egui::Context::default(), false);
            // Cancellation/EOF can have occurred, but cleanup still owns the
            // connection. Arming the notice cannot publish a terminal result.
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            terminal.complete(outcome.clone());
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            drop(terminal);
            let receipt = next_owner_receipt(&rx, owner);
            match (&outcome, receipt.outcome) {
                (Ok(()), OwnerOutcome::Verified) => {}
                (Err(expected), OwnerOutcome::Unverified(actual)) => assert_eq!(*expected, actual),
                _ => panic!("consuming close outcome changed in owner publication"),
            }
            let WorkerEvent::Closed { generation, result } = rx.try_recv().unwrap() else {
                panic!("cleanup must retain its legacy terminal event after the owner receipt");
            };
            assert_eq!(generation, 11);
            assert_eq!(result, outcome);
            assert!(matches!(
                rx.try_recv(),
                Err(mpsc::TryRecvError::Disconnected)
            ));
        }
    }

    #[test]
    fn unwind_drops_connection_first_and_overrides_staged_success() {
        struct ConnectionOwner<'a> {
            events: &'a mpsc::Receiver<WorkerEvent>,
            dropped: Arc<AtomicBool>,
        }
        impl Drop for ConnectionOwner<'_> {
            fn drop(&mut self) {
                assert!(matches!(
                    self.events.try_recv(),
                    Err(mpsc::TryRecvError::Empty)
                ));
                self.dropped.store(true, Ordering::SeqCst);
            }
        }

        // Default uncertainty, staged NoChild and staged Verified all become
        // Unverified if unwinding starts before normal terminal publication.
        for stage in 0..3 {
            let (tx, rx) = mpsc::channel();
            let owner = registered_owner(15);
            let dropped = Arc::new(AtomicBool::new(false));
            let terminal = TerminalNotice::new(owner, tx, egui::Context::default(), false);
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Match the worker closure: captured notice rebound before Client.
                let mut terminal = terminal;
                let _client = ConnectionOwner {
                    events: &rx,
                    dropped: dropped.clone(),
                };
                if stage == 1 {
                    terminal.complete_failure(ConnectionFailure {
                        message: "controlled failure".into(),
                        ownership: ConnectionOwnership::NoChild,
                        cleanup_error: None,
                    });
                } else if stage == 2 {
                    terminal.complete(Ok(()));
                }
                panic!("controlled established-worker unwind");
            }));
            assert!(panic.is_err());
            assert!(dropped.load(Ordering::SeqCst));
            assert!(matches!(
                next_owner_receipt(&rx, owner).outcome,
                OwnerOutcome::Unverified(ref reason) if reason == CLEANUP_UNVERIFIED
            ));
            let WorkerEvent::Closed { generation, result } = rx.try_recv().unwrap() else {
                panic!("worker unwind must retain legacy unverified cleanup event");
            };
            assert_eq!(generation, 15);
            assert_eq!(result.unwrap_err(), CLEANUP_UNVERIFIED);
            assert!(matches!(
                rx.try_recv(),
                Err(mpsc::TryRecvError::Disconnected)
            ));
        }
    }

    #[test]
    fn recording_mailbox_drop_does_not_settle_registered_owner() {
        let (worker, rx) = Worker::recording();
        assert_eq!(worker.owner_ticket(), None);
        drop(worker);
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));

        let mut ledger = OwnerLedger::default();
        let owner = ledger.register(16).unwrap();
        let (worker, rx) = Worker::recording_with_owner(owner);
        assert_eq!(worker.owner_ticket(), Some(owner));
        drop(worker);
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(!ledger.all_verified());
    }

    #[test]
    fn closed_result_receiver_does_not_panic_during_terminal_notification() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let mut terminal =
            TerminalNotice::new(registered_owner(17), tx, egui::Context::default(), false);
        terminal.complete(Ok(()));
        drop(terminal);
    }
}
