//! Every connect and request executes off the UI thread. Old generations are ignored.
use cedar_client::{Client, ConnectionCancellation, ConnectionSpec};
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
pub enum WorkerEvent {
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

// Declare this before the established Client so unwinding drops that owner
// before reporting unverified cleanup. Only a consuming close can replace the
// fallback with success; losing a mailbox or observing EOF is not completion.
struct TerminalNotice {
    generation: u64,
    result_tx: Sender<WorkerEvent>,
    ctx: egui::Context,
    result: Option<Result<(), String>>,
}
impl TerminalNotice {
    fn new(generation: u64, result_tx: Sender<WorkerEvent>, ctx: egui::Context) -> Self {
        Self {
            generation,
            result_tx,
            ctx,
            result: None,
        }
    }

    fn arm(&mut self) {
        self.result = Some(Err(
            "transport_cleanup_unverified: connection worker exited before cleanup was confirmed"
                .into(),
        ));
    }

    fn complete(&mut self, result: Result<(), String>) {
        self.result = Some(result);
    }
}
impl Drop for TerminalNotice {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = self.result_tx.send(WorkerEvent::Closed {
                generation: self.generation,
                result,
            });
            self.ctx.request_repaint();
        }
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
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Worker {
    #[cfg(test)]
    pub(crate) fn recording() -> (Self, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx: CommandSender {
                    inner: CommandChannel::Recording(tx),
                },
                cancel: ConnectionCancellation::new(),
            },
            rx,
        )
    }
    pub fn spawn(
        spec: ConnectionSpec,
        generation: u64,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
    ) -> Self {
        Self::spawn_with_connection(
            move |cancel| Client::connect_with_cancellation(spec, cancel),
            generation,
            result_tx,
            ctx,
            Duration::from_secs(3),
        )
    }

    // Only tests can choose a peer binary. Exercise the production connection,
    // cancellation and request loop without changing shipped agent resolution.
    #[cfg(test)]
    pub(crate) fn spawn_agent(
        agent: std::path::PathBuf,
        root: std::path::PathBuf,
        generation: u64,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
    ) -> Self {
        Self::spawn_with_connection(
            move |cancel| Client::spawn_agent_with_cancellation(&agent, &root, false, cancel),
            generation,
            result_tx,
            ctx,
            Duration::from_secs(3),
        )
    }

    // Shorten only the cleanup observation window in real-process tests. The
    // Client still consumes its exact owner through the ordinary close path;
    // its reaper grace and eventual responsibility are unchanged.
    #[cfg(test)]
    pub(crate) fn spawn_agent_with_close_timeout(
        agent: std::path::PathBuf,
        root: std::path::PathBuf,
        generation: u64,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
        timeout: Duration,
    ) -> Self {
        Self::spawn_with_connection(
            move |cancel| Client::spawn_agent_with_cancellation(&agent, &root, false, cancel),
            generation,
            result_tx,
            ctx,
            timeout,
        )
    }

    fn spawn_with_connection(
        connect: impl FnOnce(ConnectionCancellation) -> Result<Client, String> + Send + 'static,
        generation: u64,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
        close_timeout: Duration,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let tx = Arc::new(tx);
        // The transport reader must not keep its worker's mailbox alive. Once
        // Worker drops, recv ends even when the peer stays silently connected.
        let wake_tx = Arc::downgrade(&tx);
        let cancel = ConnectionCancellation::new();
        let cancelled = cancel.clone();
        std::thread::spawn(move || {
            let mut terminal = TerminalNotice::new(generation, result_tx.clone(), ctx.clone());
            let mut client = match connect(cancelled.clone()) {
                Ok(client) => client,
                Err(error) => {
                    let _ = result_tx.send(WorkerEvent::Response(Event {
                        generation,
                        id: 0,
                        connected: false,
                        result: Err(error),
                    }));
                    ctx.request_repaint();
                    return;
                }
            };
            terminal.arm();
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
        Self {
            tx: CommandSender {
                inner: CommandChannel::Worker(tx),
            },
            cancel,
        }
    }
}

#[cfg(test)]
mod terminal_notice_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn failed_connection_has_no_established_cleanup_event() {
        let (tx, rx) = mpsc::channel();
        drop(TerminalNotice::new(7, tx, egui::Context::default()));
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
            let mut terminal = TerminalNotice::new(11, tx, egui::Context::default());
            terminal.arm();
            // Cancellation/EOF can have occurred, but cleanup still owns the
            // connection. Arming the notice cannot publish a terminal result.
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            terminal.complete(outcome.clone());
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            drop(terminal);
            let WorkerEvent::Closed { generation, result } = rx.try_recv().unwrap() else {
                panic!("cleanup must have its own terminal event");
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
    fn unwind_drops_connection_before_reporting_unverified_cleanup() {
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

        let (tx, rx) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Match the worker's ownership order: notice before Client.
            let mut terminal = TerminalNotice::new(13, tx, egui::Context::default());
            let _client = ConnectionOwner {
                events: &rx,
                dropped: dropped.clone(),
            };
            terminal.arm();
            panic!("controlled established-worker unwind");
        }));
        assert!(panic.is_err());
        assert!(dropped.load(Ordering::SeqCst));
        let WorkerEvent::Closed { generation, result } = rx.try_recv().unwrap() else {
            panic!("worker unwind must report unverified cleanup");
        };
        assert_eq!(generation, 13);
        assert_eq!(
            result.unwrap_err(),
            "transport_cleanup_unverified: connection worker exited before cleanup was confirmed"
        );
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn closed_result_receiver_does_not_panic_during_terminal_notification() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let mut terminal = TerminalNotice::new(17, tx, egui::Context::default());
        terminal.arm();
        terminal.complete(Ok(()));
        drop(terminal);
    }
}
