//! Existing blocking stdio implementation for non-Windows hosts.
use super::{route_message, ClientOptions, Error, ProcessConfig, Shared, WriteCommand};
use crate::framing::{read_frame, write_frame};
use serde_json::Value;
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(super) struct Backend {
    child: Mutex<Child>,
}

impl Backend {
    pub(super) fn spawn(
        config: ProcessConfig,
        options: &ClientOptions,
        shared: Arc<Shared>,
        outbound: mpsc::SyncSender<WriteCommand>,
        writes: mpsc::Receiver<WriteCommand>,
    ) -> Result<Self, Error> {
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        command.stderr(if config.inherit_stderr {
            Stdio::inherit()
        } else {
            Stdio::null()
        });
        if let Some(directory) = &config.working_directory {
            command.current_dir(directory);
        }
        let mut child = command
            .spawn()
            .map_err(|e| Error::Io(format!("launch {}: {e}", config.program.display())))?;
        // Stdio::piped guarantees these handles after a successful spawn.
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        // Own the child before fallible thread creation, including unwind.
        let backend = Self {
            child: Mutex::new(child),
        };
        let writer_shared = Arc::clone(&shared);
        let limits = options.frame_limits;
        thread::Builder::new()
            .name("cedar-lsp-writer".into())
            .spawn(move || {
                while let Ok(write) = writes.recv() {
                    if writer_shared.routing.lock().unwrap().terminal.is_some() {
                        break;
                    }
                    if Instant::now() >= write.deadline {
                        if let Some(ack) = write.ack {
                            let _ = ack.try_send(Err(Error::Timeout("stdio write".into())));
                        }
                        continue;
                    }
                    let result = write_frame(&mut stdin, &write.bytes, limits)
                        .map_err(|e| Error::Io(e.to_string()));
                    if write.close_stdin {
                        // Exit is the final LSP message. EOF releases servers
                        // whose listener remains blocked during orderly exit.
                        drop(stdin);
                        if let Some(ack) = write.ack {
                            let _ = ack.try_send(result.clone());
                        }
                        if let Err(error) = result {
                            writer_shared.fail(error);
                        }
                        return;
                    }
                    if let Some(ack) = write.ack {
                        let _ = ack.try_send(result.clone());
                    }
                    if let Err(error) = result {
                        writer_shared.fail(error);
                        break;
                    }
                }
            })
            .map_err(|e| Error::Io(format!("start writer: {e}")))?;
        let reader_timeout = options.request_timeout;
        thread::Builder::new()
            .name("cedar-lsp-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let message = match read_frame(&mut reader, limits) {
                        Ok(Some(bytes)) => match serde_json::from_slice::<Value>(&bytes) {
                            Ok(value) => value,
                            Err(e) => {
                                shared.fail(Error::Protocol(format!("invalid JSON: {e}")));
                                break;
                            }
                        },
                        Ok(None) => {
                            shared.fail(Error::Closed("server stdout reached EOF".into()));
                            break;
                        }
                        Err(e) => {
                            shared.fail(Error::Protocol(e.to_string()));
                            break;
                        }
                    };
                    if let Err(error) =
                        route_message(message, &shared, &outbound, reader_timeout, limits)
                    {
                        shared.fail(error);
                        break;
                    }
                }
            })
            .map_err(|e| Error::Io(format!("start reader: {e}")))?;
        Ok(backend)
    }

    pub(super) fn process_id(&self) -> u32 {
        self.child.lock().unwrap().id()
    }

    pub(super) fn wake(&self) {}

    pub(super) fn begin_shutdown(&self, _timeout: Duration) {}

    pub(super) fn shutdown_outcome(&self) -> Option<crate::WindowsShutdownOutcome> {
        None
    }

    pub(super) fn transport_failed(&self) {}

    pub(super) fn begin_abort(&self) {}

    pub(super) fn finish(&self, shared: &Shared, timeout: Duration) -> Result<(), Error> {
        let deadline = Instant::now() + timeout;
        loop {
            let status = self.child.lock().unwrap().try_wait();
            match status {
                Ok(Some(_)) => {
                    shared.fail(Error::Closed("server exited".into()));
                    return Ok(());
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    shared.fail(Error::Closed("shutdown grace period elapsed".into()));
                    self.abort();
                    return Ok(());
                }
                Err(e) => return Err(Error::Io(format!("wait for server: {e}"))),
            }
        }
    }

    pub(super) fn abort(&self) {
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.abort();
        // Preserve the portable behavior: inherited descendant pipes can block
        // these legacy threads, so only the direct child is killed and reaped.
    }
}
