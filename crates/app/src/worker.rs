//! Every connect and request executes off the UI thread. Old generations are ignored.
use cedar_client::{Client, ConnectionCancellation, ConnectionSpec};
use cedar_protocol::{Operation, Payload};
use eframe::egui;
use std::sync::mpsc::{self, Receiver, Sender};

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
pub struct Worker {
    pub tx: Sender<Command>,
    cancel: ConnectionCancellation,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Worker {
    #[cfg(test)]
    pub(crate) fn recording() -> (Self, Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                cancel: ConnectionCancellation::new(),
            },
            rx,
        )
    }
    pub fn spawn(
        spec: ConnectionSpec,
        generation: u64,
        result_tx: Sender<Event>,
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
        result_tx: Sender<Event>,
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
        result_tx: Sender<Event>,
        ctx: egui::Context,
    ) -> Self {
        let (tx, rx): (Sender<Command>, Receiver<Command>) = mpsc::channel();
        let cancel = ConnectionCancellation::new();
        let cancelled = cancel.clone();
        std::thread::spawn(move || {
            let mut client = match connect(cancelled.clone()) {
                Ok(client) => client,
                Err(error) => {
                    let _ = result_tx.send(Event {
                        generation,
                        id: 0,
                        connected: false,
                        result: Err(error),
                    });
                    ctx.request_repaint();
                    return;
                }
            };
            if cancelled.is_cancelled() {
                return;
            }
            let hello = client.handshake().clone();
            let _ = result_tx.send(Event {
                generation,
                id: 0,
                connected: true,
                result: Ok(hello),
            });
            ctx.request_repaint();
            while let Ok(command) = rx.recv() {
                if cancelled.is_cancelled() {
                    break;
                }
                let result = client.request(command.op);
                if result_tx
                    .send(Event {
                        generation,
                        id: command.id,
                        connected: client.is_connected(),
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { tx, cancel }
    }
}
