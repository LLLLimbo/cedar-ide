//! Every connect and request executes off the UI thread. Old generations are ignored.
use cedar_client::{Client, ConnectionCancellation, ConnectionSpec};
use cedar_protocol::{Operation, Payload};
use eframe::egui;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

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
    TransportLost { generation: u64, message: String },
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
        )
    }

    fn spawn_with_connection(
        connect: impl FnOnce(ConnectionCancellation) -> Result<Client, String> + Send + 'static,
        generation: u64,
        result_tx: Sender<WorkerEvent>,
        ctx: egui::Context,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let tx = Arc::new(tx);
        // The transport reader must not keep its worker's mailbox alive. Once
        // Worker drops, recv ends even when the peer stays silently connected.
        let wake_tx = Arc::downgrade(&tx);
        let cancel = ConnectionCancellation::new();
        let cancelled = cancel.clone();
        std::thread::spawn(move || {
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
            if cancelled.is_cancelled() {
                return;
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
                return;
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
            client.clear_transport_waker();
        });
        Self {
            tx: CommandSender {
                inner: CommandChannel::Worker(tx),
            },
            cancel,
        }
    }
}
