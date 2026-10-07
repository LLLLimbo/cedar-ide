//! Every connect and request executes off the UI thread. Old generations are ignored.
use cedar_client::{Client, ConnectionSpec};
use cedar_protocol::{Operation, Payload};
use eframe::egui;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};

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
    cancel: Arc<AtomicBool>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Worker {
    #[cfg(test)]
    pub(crate) fn recording() -> (Self, Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                cancel: Arc::new(AtomicBool::new(false)),
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
        let (tx, rx): (Sender<Command>, Receiver<Command>) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let mut client = match Client::connect(spec) {
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
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            let hello = client.request(Operation::Hello);
            let failed = hello.is_err();
            let _ = result_tx.send(Event {
                generation,
                id: 0,
                connected: !failed,
                result: hello,
            });
            ctx.request_repaint();
            if failed {
                return;
            }
            while let Ok(command) = rx.recv() {
                if cancelled.load(Ordering::Acquire) {
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
