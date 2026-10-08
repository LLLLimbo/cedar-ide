//! Platform-independent state used by the owned Windows worker, with deterministic
//! tests on every host. No process or OS I/O is performed here.
use super::{Error, WriteCommand};
use crate::framing::{FrameLimits, IncrementalDecoder};
use std::time::{Duration, Instant};

pub(super) struct ActiveWrite {
    command: WriteCommand,
    header: Vec<u8>,
    written: usize,
    submitted: usize,
    started: bool,
}

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

pub(super) const STDERR_TAIL_BYTES: usize = 16 * 1024;

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
