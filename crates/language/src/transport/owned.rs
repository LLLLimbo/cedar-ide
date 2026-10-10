//! Platform-independent state used by the owned Windows worker, with deterministic
//! tests on every host. No process or OS I/O is performed here.
use super::Error;
#[cfg(any(windows, test))]
use super::WriteCommand;
use crate::framing::{FrameLimits, IncrementalDecoder};
#[cfg(any(windows, test))]
use crate::{
    WindowsCleanupErrors, WindowsCleanupStatus, WindowsRootExit, WindowsShutdownOutcome,
    WindowsShutdownReason,
};
#[cfg(any(windows, test))]
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The caller serializes joiners while the handle is consumed. A consumed handle
/// must retain its failure; its absence must never be interpreted as success.
#[cfg(any(windows, test))]
pub(super) enum DurableJoin<T> {
    Running(JoinHandle<T>),
    Finished(Result<T, ()>),
}

#[cfg(any(windows, test))]
impl<T: Clone> DurableJoin<T> {
    pub(super) fn join(&mut self) -> Result<T, ()> {
        if matches!(self, Self::Running(_)) {
            let Self::Running(worker) = std::mem::replace(self, Self::Finished(Err(()))) else {
                unreachable!();
            };
            *self = Self::Finished(worker.join().map_err(|_| ()));
        }
        match self {
            Self::Finished(result) => result.clone(),
            Self::Running(_) => unreachable!(),
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct TerminalState {
    pub(super) reason: Option<WindowsShutdownReason>,
    pub(super) transport_failure_observed: bool,
    pub(super) root_exit: WindowsRootExit,
    pub(super) errors: WindowsCleanupErrors,
}

#[cfg(any(windows, test))]
impl TerminalState {
    pub(super) fn record_reason(&mut self, reason: WindowsShutdownReason) {
        self.reason.get_or_insert(reason);
    }

    pub(super) fn transport_failed(&mut self) {
        self.transport_failure_observed = true;
        self.record_reason(WindowsShutdownReason::TransportFailure);
    }

    // Called only after the ownership thread has actually been joined.
    pub(super) fn joined(&mut self, panicked: bool) -> WindowsShutdownOutcome {
        if panicked {
            self.record_reason(WindowsShutdownReason::WorkerPanicked);
            self.errors.worker_panicked = true;
        }
        WindowsShutdownOutcome {
            reason: self
                .reason
                .unwrap_or(WindowsShutdownReason::TransportFailure),
            transport_failure_observed: self.transport_failure_observed,
            root_exit: self.root_exit,
            cleanup: if panicked {
                WindowsCleanupStatus::Unverified
            } else if self.errors == WindowsCleanupErrors::default() {
                WindowsCleanupStatus::Joined
            } else {
                WindowsCleanupStatus::JoinedWithErrors
            },
            errors: self.errors,
        }
    }
}

#[cfg(any(windows, test))]
pub(super) struct ActiveWrite {
    command: WriteCommand,
    header: Vec<u8>,
    written: usize,
    submitted: usize,
    started: bool,
}

#[cfg(any(windows, test))]
impl ActiveWrite {
    pub(super) fn new(command: WriteCommand, limits: FrameLimits) -> Result<Self, Error> {
        let header = format!("Content-Length: {}\r\n\r\n", command.bytes.len()).into_bytes();
        if header.len() > limits.max_header_bytes || command.bytes.len() > limits.max_content_bytes
        {
            return Err(Error::Protocol(
                "outbound frame exceeds configured limit".into(),
            ));
        }
        Ok(Self {
            command,
            header,
            written: 0,
            submitted: 0,
            started: false,
        })
    }

    pub(super) fn expired(&self, now: Instant) -> bool {
        now >= self.command.deadline
    }

    // Once submission begins, even an unobserved completion may have sent bytes.
    pub(super) fn started(&self) -> bool {
        self.started
    }
    pub(super) fn pending(&self) -> bool {
        self.submitted != 0
    }
    pub(super) fn complete(&self) -> bool {
        self.written == self.header.len() + self.command.bytes.len()
    }

    pub(super) fn begin_chunk(&mut self, max_bytes: usize) -> &[u8] {
        debug_assert!(!self.pending() && !self.complete() && max_bytes > 0);
        let bytes = if self.written < self.header.len() {
            &self.header[self.written..]
        } else {
            &self.command.bytes[self.written - self.header.len()..]
        };
        self.submitted = bytes.len().min(max_bytes);
        self.started = true;
        &bytes[..self.submitted]
    }

    pub(super) fn advance(&mut self, bytes: usize) -> Result<(), Error> {
        if bytes == 0 || bytes > self.submitted {
            return Err(Error::Io(
                "stdin completion returned an invalid byte count".into(),
            ));
        }
        self.written += bytes;
        self.submitted = 0;
        Ok(())
    }

    pub(super) fn closes_stdin(&self) -> bool {
        self.command.close_stdin
    }

    pub(super) fn finish(self, result: Result<(), Error>) {
        if let Some(ack) = self.command.ack {
            let _ = ack.try_send(result);
        }
    }
}

pub(super) struct Incoming {
    decoder: IncrementalDecoder,
    started: Option<Instant>,
    timeout: Duration,
}

impl Incoming {
    pub(super) fn new(limits: FrameLimits, timeout: Duration) -> Self {
        Self {
            decoder: IncrementalDecoder::new(limits),
            started: None,
            timeout,
        }
    }

    pub(super) fn check_deadline(&self, now: Instant) -> Result<(), Error> {
        if self
            .started
            .is_some_and(|start| now.saturating_duration_since(start) >= self.timeout)
        {
            return Err(Error::Timeout("server frame assembly".into()));
        }
        Ok(())
    }

    pub(super) fn push(
        &mut self,
        input: &mut &[u8],
        now: Instant,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.check_deadline(now)?;
        if !input.is_empty() && self.decoder.is_at_boundary() {
            self.started = Some(now);
        }
        let result = self
            .decoder
            .push(input)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        if self.decoder.is_at_boundary() {
            self.started = None;
        }
        Ok(result)
    }

    pub(super) fn eof(&self) -> Error {
        match self.decoder.finish() {
            Ok(()) => Error::Closed("server stdout reached EOF".into()),
            Err(e) => Error::Protocol(e.to_string()),
        }
    }
}

#[cfg(any(windows, test))]
pub(super) const STDERR_TAIL_BYTES: usize = 16 * 1024;

#[cfg(any(windows, test))]
pub(super) fn retain_tail(tail: &mut Vec<u8>, bytes: &[u8]) {
    if tail.capacity() < STDERR_TAIL_BYTES {
        tail.reserve_exact(STDERR_TAIL_BYTES - tail.len());
    }
    if bytes.len() >= STDERR_TAIL_BYTES {
        tail.clear();
        tail.extend_from_slice(&bytes[bytes.len() - STDERR_TAIL_BYTES..]);
    } else {
        let remove = (tail.len() + bytes.len()).saturating_sub(STDERR_TAIL_BYTES);
        tail.drain(..remove);
        tail.extend_from_slice(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn repeated_join_preserves_worker_panic_and_success() {
        let mut failed = DurableJoin::Running(std::thread::spawn(|| -> u8 {
            panic!("owned worker panic regression");
        }));
        assert_eq!(failed.join(), Err(()));
        assert_eq!(failed.join(), Err(()));
        let mut success = DurableJoin::Running(std::thread::spawn(|| 7));
        assert_eq!(success.join(), Ok(7));
        assert_eq!(success.join(), Ok(7));
    }

    #[test]
    fn final_malformed_output_after_root_exit_cannot_be_graceful() {
        // The decoder can receive trailing bytes on the mandatory final capture
        // after root exit was observed. Exercise both orderings deterministically.
        for failure_first in [false, true] {
            for truncated in [false, true] {
                let mut terminal = TerminalState::default();
                if !failure_first {
                    terminal.record_reason(WindowsShutdownReason::RootExited);
                    terminal.root_exit = WindowsRootExit::BeforeTermination(0);
                }
                let mut incoming = Incoming::new(FrameLimits::default(), Duration::from_secs(1));
                if truncated {
                    incoming
                        .push(&mut &b"Content-Length: 4\r\n\r\n{"[..], Instant::now())
                        .unwrap();
                    assert!(matches!(incoming.eof(), Error::Protocol(_)));
                } else {
                    assert!(matches!(
                        incoming.push(&mut &b"Content-Length: -1\r\n\r\n"[..], Instant::now()),
                        Err(Error::Protocol(_))
                    ));
                }
                terminal.transport_failed();
                terminal.record_reason(WindowsShutdownReason::RootExited);
                terminal.root_exit = WindowsRootExit::BeforeTermination(0);
                let owned = terminal.joined(false);
                assert!(owned.transport_failure_observed);
                assert_eq!(
                    owned.reason,
                    if failure_first {
                        WindowsShutdownReason::TransportFailure
                    } else {
                        WindowsShutdownReason::RootExited
                    }
                );
                assert_eq!(owned.cleanup, WindowsCleanupStatus::Joined);
                let outcome = crate::ShutdownOutcome {
                    shutdown_response_received: true,
                    exit_frame_completed: true,
                    windows: Some(owned),
                    linux: None,
                };
                assert!(!outcome.is_graceful());
                terminal.record_reason(WindowsShutdownReason::Aborted);
                assert_eq!(terminal.joined(false), owned);
            }
        }
    }

    fn write(now: Instant) -> (ActiveWrite, mpsc::Receiver<Result<(), Error>>) {
        let (ack, recv) = mpsc::sync_channel(1);
        let command = WriteCommand {
            close_stdin: false,
            bytes: b"abc".to_vec(),
            deadline: now,
            ack: Some(ack),
        };
        (
            ActiveWrite::new(command, FrameLimits::default()).unwrap(),
            recv,
        )
    }

    #[test]
    fn final_exit_write_retains_close_intent_until_all_bytes_complete() {
        let (mut active, _ack) = write(Instant::now() + Duration::from_secs(1));
        assert!(!active.closes_stdin());
        active.command.close_stdin = true;
        assert!(active.closes_stdin());
        assert!(!active.complete());
        while !active.complete() {
            let count = active.begin_chunk(2).len();
            active.advance(count).unwrap();
        }
        assert!(active.closes_stdin());
    }

    #[test]
    fn short_completions_send_each_byte_once_and_ack_only_complete_frame() {
        let (mut active, ack) = write(Instant::now() + Duration::from_secs(1));
        let mut transported = Vec::new();
        while !active.complete() {
            let chunk = active.begin_chunk(4);
            transported.push(chunk[0]); // deterministically force short writes
            assert!(ack.try_recv().is_err());
            assert!(active.pending());
            active.advance(1).unwrap();
        }
        assert_eq!(transported, b"Content-Length: 3\r\n\r\nabc");
        active.finish(Ok(()));
        ack.try_recv().unwrap().unwrap();
    }

    #[test]
    fn expired_queued_write_is_distinct_from_unknown_partial_delivery() {
        let now = Instant::now();
        let (mut active, _) = write(now);
        assert!(active.expired(now));
        assert!(!active.started());
        active.begin_chunk(2);
        assert!(active.started() && active.pending());
        // No completion was observed, yet cancelling cannot prove zero delivery.
        assert!(active.expired(now + Duration::from_secs(1)));
        active.advance(1).unwrap();
        assert!(active.started() && !active.pending() && !active.complete());
    }

    #[test]
    fn rejects_impossible_completion_counts() {
        let (mut active, _) = write(Instant::now());
        assert!(active.advance(1).is_err());
        active.begin_chunk(2);
        assert!(active.advance(0).is_err());
        assert!(active.advance(3).is_err());
        active.advance(2).unwrap();
    }

    #[test]
    fn frame_timer_ignores_idle_and_does_not_reset_for_trickle_bytes() {
        let now = Instant::now();
        let limit = Duration::from_secs(1);
        let mut incoming = Incoming::new(FrameLimits::default(), limit);
        incoming.check_deadline(now + limit * 100).unwrap();
        incoming.push(&mut &b"C"[..], now).unwrap();
        incoming.push(&mut &b"o"[..], now + limit / 2).unwrap();
        assert!(matches!(
            incoming.check_deadline(now + limit),
            Err(Error::Timeout(_))
        ));
    }

    #[test]
    fn completed_frame_resets_timer_and_partial_eof_is_terminal() {
        let now = Instant::now();
        let limit = Duration::from_secs(1);
        let mut incoming = Incoming::new(FrameLimits::default(), limit);
        assert!(matches!(incoming.eof(), Error::Closed(_)));
        incoming
            .push(&mut &b"Content-Length: 2\r\n\r\n{"[..], now)
            .unwrap();
        assert!(matches!(incoming.eof(), Error::Protocol(_)));
        assert_eq!(
            incoming
                .push(&mut &b"}"[..], now + limit / 2)
                .unwrap()
                .unwrap(),
            b"{}"
        );
        incoming.check_deadline(now + limit * 100).unwrap();
        assert!(matches!(incoming.eof(), Error::Closed(_)));
    }

    #[test]
    fn stderr_retention_is_exactly_the_bounded_suffix() {
        let mut tail = Vec::new();
        retain_tail(&mut tail, &[b'a'; STDERR_TAIL_BYTES - 2]);
        retain_tail(&mut tail, b"bcde");
        assert_eq!(tail.len(), STDERR_TAIL_BYTES);
        assert_eq!(&tail[tail.len() - 4..], b"bcde");
        retain_tail(&mut tail, &[b'x'; STDERR_TAIL_BYTES + 1]);
        assert_eq!(tail, vec![b'x'; STDERR_TAIL_BYTES]);
    }
}
