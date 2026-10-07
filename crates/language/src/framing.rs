//! Content-Length framing shared by LSP and DAP. No unbounded header reads.

use std::io::{self, BufRead, Write};
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct FrameLimits {
    pub max_header_bytes: usize,
    pub max_content_bytes: usize,
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 8 * 1024,
            max_content_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("stdio I/O: {0}")]
    Io(#[from] io::Error),
    #[error("JSON serialization: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid protocol frame: {0}")]
    Invalid(&'static str),
    #[error("frame header exceeds configured limit")]
    HeaderTooLarge,
    #[error("frame body ({actual} bytes) exceeds configured limit ({limit})")]
    ContentTooLarge { actual: usize, limit: usize },
}

/// Read one UTF-8 JSON payload's bytes, or `None` on EOF at a frame boundary.
/// Partial headers/bodies are errors. Retained header bytes never exceed the limit.
/// Content-Type is optional; only UTF-8 (including legacy `utf8`) is accepted.
pub fn read_frame<R: BufRead>(
    reader: &mut R,
    limits: FrameLimits,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if header.is_empty() {
                Ok(None)
            } else {
                Err(FrameError::Invalid("EOF inside header"))
            };
        }
        let mut consumed = 0;
        let mut complete = false;
        for &byte in available {
            if header.len() >= limits.max_header_bytes {
                return Err(FrameError::HeaderTooLarge);
            }
            if !byte.is_ascii() {
                return Err(FrameError::Invalid("header must be ASCII"));
            }
            header.push(byte);
            consumed += 1;
            if header.ends_with(b"\r\n\r\n") {
                complete = true;
                break;
            }
        }
        reader.consume(consumed);
        if complete {
            break;
        }
    }
    let length = parse_header(&header, limits)?;
    let mut content = vec![0; length];
    reader.read_exact(&mut content)?;
    Ok(Some(content))
}

/// Parse a complete ASCII header, including its terminating empty line.
fn parse_header(header: &[u8], limits: FrameLimits) -> Result<usize, FrameError> {
    let text =
        std::str::from_utf8(header).map_err(|_| FrameError::Invalid("header must be ASCII"))?;
    let mut length = None;
    for line in text[..text.len() - 4].split("\r\n") {
        let (name, value) = line
            .split_once(':')
            .ok_or(FrameError::Invalid("malformed header field"))?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(FrameError::Invalid("malformed header name"));
        }
        if value.bytes().any(|b| b.is_ascii_control() && b != b'\t') {
            return Err(FrameError::Invalid("control character in header value"));
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err(FrameError::Invalid("duplicate Content-Length"));
            }
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(FrameError::Invalid("invalid Content-Length"));
            }
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| FrameError::Invalid("Content-Length overflow"))?,
            );
        } else if name.eq_ignore_ascii_case("Content-Type") {
            for param in value.split(';').skip(1) {
                let (key, value) = param
                    .trim()
                    .split_once('=')
                    .ok_or(FrameError::Invalid("malformed Content-Type"))?;
                if key.trim().eq_ignore_ascii_case("charset") {
                    let charset = value.trim().trim_matches('"');
                    if !charset.eq_ignore_ascii_case("utf-8")
                        && !charset.eq_ignore_ascii_case("utf8")
                    {
                        return Err(FrameError::Invalid("only UTF-8 content is supported"));
                    }
                }
            }
        }
    }
    let length = length.ok_or(FrameError::Invalid("missing Content-Length"))?;
    if length > limits.max_content_bytes {
        return Err(FrameError::ContentTooLarge {
            actual: length,
            limit: limits.max_content_bytes,
        });
    }
    Ok(length)
}

/// Assemble frames without blocking on a partial header or body.
///
/// Input is consumed through at most one frame per call. The caller owns read
/// deadlines and must discard the decoder after an error. Body bytes are opaque;
/// Content-Length counts bytes, including each byte of a UTF-8 character.
#[cfg(any(windows, test))]
pub(crate) struct IncrementalDecoder {
    limits: FrameLimits,
    state: DecodeState,
}

#[cfg(any(windows, test))]
enum DecodeState {
    Header(Vec<u8>),
    Body { length: usize, bytes: Vec<u8> },
}

#[cfg(any(windows, test))]
impl IncrementalDecoder {
    pub(crate) fn new(limits: FrameLimits) -> Self {
        Self {
            limits,
            state: DecodeState::Header(Vec::new()),
        }
    }

    pub(crate) fn push(&mut self, input: &mut &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
        loop {
            match &mut self.state {
                DecodeState::Header(header) => {
                    loop {
                        let Some((&byte, rest)) = input.split_first() else {
                            return Ok(None);
                        };
                        if header.len() >= self.limits.max_header_bytes {
                            return Err(FrameError::HeaderTooLarge);
                        }
                        if !byte.is_ascii() {
                            return Err(FrameError::Invalid("header must be ASCII"));
                        }
                        reserve_bounded(header, 1, self.limits.max_header_bytes)?;
                        header.push(byte);
                        *input = rest;
                        if header.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let length = parse_header(header, self.limits)?;
                    // Release the header before retaining any body bytes. In
                    // particular, do not reserve the advertised body length:
                    // even a permissive limit must not let a header alone
                    // trigger an enormous allocation.
                    self.state = DecodeState::Body {
                        length,
                        bytes: Vec::new(),
                    };
                }
                DecodeState::Body { length, bytes } => {
                    let count = input.len().min(*length - bytes.len());
                    reserve_bounded(bytes, count, *length)?;
                    bytes.extend_from_slice(&input[..count]);
                    *input = &input[count..];
                    if bytes.len() == *length {
                        let content = std::mem::take(bytes);
                        self.state = DecodeState::Header(Vec::new());
                        return Ok(Some(content));
                    }
                    return Ok(None);
                }
            }
        }
    }

    pub(crate) fn is_at_boundary(&self) -> bool {
        matches!(&self.state, DecodeState::Header(header) if header.is_empty())
    }

    /// Check EOF separately from an empty input fragment.
    pub(crate) fn finish(&self) -> Result<(), FrameError> {
        match &self.state {
            DecodeState::Header(header) if header.is_empty() => Ok(()),
            DecodeState::Header(_) => Err(FrameError::Invalid("EOF inside header")),
            DecodeState::Body { .. } => Err(FrameError::Invalid("EOF inside body")),
        }
    }
}

/// Grow geometrically, but never reserve beyond the validated limit. Arithmetic
/// depends on bytes actually received, so usize::MAX limits cannot overflow it.
fn reserve_bounded(bytes: &mut Vec<u8>, additional: usize, limit: usize) -> Result<(), FrameError> {
    // Callers limit `additional` to the space left in the header or body.
    debug_assert!(additional <= limit - bytes.len());
    let required = bytes.len() + additional;
    if required > bytes.capacity() {
        let capacity = required.max(bytes.capacity().saturating_mul(2)).min(limit);
        bytes
            .try_reserve_exact(capacity - bytes.len())
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    }
    Ok(())
}

/// Serialize without allowing the output buffer to grow past the content limit.
/// This also bounds outgoing messages before they enter the writer queue.
pub fn encode_json<T: serde::Serialize>(
    value: &T,
    limits: FrameLimits,
) -> Result<Vec<u8>, FrameError> {
    struct LimitedBuffer {
        bytes: Vec<u8>,
        limit: usize,
        overflow: bool,
    }
    impl Write for LimitedBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                self.overflow = true;
                return Err(io::Error::other("JSON exceeds frame limit"));
            }
            // Bound reserved capacity as well as length for queued payloads.
            reserve_bounded(&mut self.bytes, bytes.len(), self.limit).map_err(io::Error::other)?;
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = LimitedBuffer {
        bytes: Vec::new(),
        limit: limits.max_content_bytes,
        overflow: false,
    };
    let result = serde_json::to_writer(&mut writer, value);
    if writer.overflow {
        return Err(FrameError::ContentTooLarge {
            actual: limits.max_content_bytes.saturating_add(1),
            limit: limits.max_content_bytes,
        });
    }
    result?;
    Ok(writer.bytes)
}

/// Write and flush one complete frame. `content.len()` is a byte count.
pub fn write_frame<W: Write>(
    writer: &mut W,
    content: &[u8],
    limits: FrameLimits,
) -> Result<(), FrameError> {
    if content.len() > limits.max_content_bytes {
        return Err(FrameError::ContentTooLarge {
            actual: content.len(),
            limit: limits.max_content_bytes,
        });
    }
    let header = format!("Content-Length: {}\r\n\r\n", content.len());
    if header.len() > limits.max_header_bytes {
        return Err(FrameError::HeaderTooLarge);
    }
    writer.write_all(header.as_bytes())?;
    writer.write_all(content)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn unicode_concatenation_and_fragmented_reads() {
        let a = "{\"text\":\"λ🦀\"}".as_bytes();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, a, FrameLimits::default()).unwrap();
        write_frame(&mut bytes, b"null", FrameLimits::default()).unwrap();
        let mut reader = BufReader::with_capacity(1, Cursor::new(bytes));
        assert_eq!(
            read_frame(&mut reader, FrameLimits::default())
                .unwrap()
                .unwrap(),
            a
        );
        assert_eq!(
            read_frame(&mut reader, FrameLimits::default())
                .unwrap()
                .unwrap(),
            b"null"
        );
        assert!(read_frame(&mut reader, FrameLimits::default())
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_bad_headers_and_truncation() {
        for bytes in [
            "Content-Length: -1\r\n\r\n",
            "Content-Length: +1\r\n\r\nx",
            "Content-Length: 1\r\nContent-Length: 1\r\n\r\nx",
            "Other: 2\r\n\r\n{}",
            "Content-Length: 2\r\n\r\n{",
            "Content-Length: 2\r\n",
            "Content-Length: 9999999999999999999999999\r\n\r\n",
            "Content-Length: 2\r\nContent-Type: application/json; charset=latin1\r\n\r\n{}",
            "Content-Length: 2\n\n{}",
            "Bad Field: 2\r\n\r\n{}",
        ] {
            assert!(
                read_frame(&mut Cursor::new(bytes.as_bytes()), FrameLimits::default()).is_err(),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn rejects_oversized_frames_before_allocating_body() {
        let limits = FrameLimits {
            max_header_bytes: 64,
            max_content_bytes: 8,
        };
        assert!(matches!(
            read_frame(
                &mut Cursor::new(b"Content-Length: 10000000000\r\n\r\n"),
                limits
            ),
            Err(FrameError::ContentTooLarge { .. })
        ));
        assert!(matches!(
            read_frame(&mut Cursor::new(vec![b'a'; 65]), limits),
            Err(FrameError::HeaderTooLarge)
        ));
        assert!(matches!(
            write_frame(&mut Vec::new(), &[0; 9], limits),
            Err(FrameError::ContentTooLarge { .. })
        ));
    }

    #[test]
    fn outgoing_json_serialization_is_bounded_and_counts_utf8_bytes() {
        let limits = FrameLimits {
            max_header_bytes: 64,
            max_content_bytes: 6,
        };
        assert_eq!(encode_json(&"🦀", limits).unwrap().len(), 6);
        assert!(matches!(
            encode_json(&"🦀x", limits),
            Err(FrameError::ContentTooLarge { .. })
        ));
        let with_control = b"Content-Length: 2\r\nX-Header: bad\0value\r\n\r\n{}";
        assert!(read_frame(&mut Cursor::new(with_control), FrameLimits::default()).is_err());
    }

    #[test]
    fn accepts_case_insensitive_header_and_legacy_charset() {
        let bytes = b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=utf8\r\n\r\n{}";
        assert_eq!(
            read_frame(&mut Cursor::new(bytes), FrameLimits::default())
                .unwrap()
                .unwrap(),
            b"{}"
        );
    }

    fn assert_decoder_storage_is_bounded(decoder: &IncrementalDecoder) {
        match &decoder.state {
            DecodeState::Header(header) => {
                assert!(header.len() <= decoder.limits.max_header_bytes);
                assert!(header.capacity() <= decoder.limits.max_header_bytes);
            }
            DecodeState::Body { length, bytes } => {
                assert!(*length <= decoder.limits.max_content_bytes);
                assert!(bytes.len() <= *length);
                assert!(bytes.capacity() <= *length);
            }
        }
    }

    fn decode_chunk(decoder: &mut IncrementalDecoder, mut input: &[u8], frames: &mut Vec<Vec<u8>>) {
        loop {
            match decoder.push(&mut input).unwrap() {
                Some(frame) => frames.push(frame),
                None => {
                    assert!(input.is_empty());
                    break;
                }
            }
            assert_decoder_storage_is_bounded(decoder);
            if input.is_empty() {
                break;
            }
        }
        assert_decoder_storage_is_bounded(decoder);
    }

    #[test]
    fn incremental_accepts_every_split_and_one_byte_fragments() {
        let payloads: &[&[u8]] = &[b"", b"null", "{\"text\":\"λ🦀\"}".as_bytes(), b"\0\xff\x80"];
        for payload in payloads {
            let mut wire = Vec::new();
            write_frame(&mut wire, payload, FrameLimits::default()).unwrap();
            let limits = FrameLimits {
                max_header_bytes: wire.len() - payload.len(),
                max_content_bytes: payload.len(),
            };
            for split in 0..=wire.len() {
                let mut decoder = IncrementalDecoder::new(limits);
                let mut frames = Vec::new();
                assert!(decoder.is_at_boundary());
                decode_chunk(&mut decoder, &wire[..split], &mut frames);
                assert_eq!(decoder.is_at_boundary(), split == 0 || split == wire.len());
                decode_chunk(&mut decoder, &[], &mut frames);
                decode_chunk(&mut decoder, &wire[split..], &mut frames);
                assert_eq!(frames, vec![payload.to_vec()], "split {split}");
                assert!(decoder.is_at_boundary());
                decoder.finish().unwrap();
            }

            let mut decoder = IncrementalDecoder::new(limits);
            let mut frames = Vec::new();
            for byte in wire.chunks(1) {
                decode_chunk(&mut decoder, byte, &mut frames);
            }
            assert_eq!(frames, vec![payload.to_vec()]);
            assert!(decoder.is_at_boundary());
            decoder.finish().unwrap();
        }
    }

    #[test]
    fn incremental_preserves_concatenated_frames_and_partial_successor() {
        let payloads: &[&[u8]] = &[b"{}", b"", "🦀".as_bytes(), b"null", b""];
        let mut wire = Vec::new();
        let mut ends = Vec::new();
        for payload in payloads {
            write_frame(&mut wire, payload, FrameLimits::default()).unwrap();
            ends.push(wire.len());
        }
        let mut decoder = IncrementalDecoder::new(FrameLimits::default());
        let mut input = wire.as_slice();
        for (payload, end) in payloads.iter().zip(ends) {
            assert_eq!(decoder.push(&mut input).unwrap().unwrap(), *payload);
            assert_eq!(input, &wire[end..]);
            assert!(decoder.is_at_boundary());
        }
        assert!(input.is_empty());
        assert!(decoder.push(&mut input).unwrap().is_none());
        decoder.finish().unwrap();

        // Every cut in a stream can leave a partial successor, including a
        // terminator split across reads and a split inside a UTF-8 code point.
        wire.extend_from_slice(b"Content-Length: 12\r\n");
        for split in 0..=wire.len() {
            let mut decoder = IncrementalDecoder::new(FrameLimits::default());
            let mut frames = Vec::new();
            decode_chunk(&mut decoder, &wire[..split], &mut frames);
            decode_chunk(&mut decoder, &wire[split..], &mut frames);
            assert_eq!(frames, payloads);
            assert!(!decoder.is_at_boundary());
            assert!(matches!(
                decoder.finish(),
                Err(FrameError::Invalid("EOF inside header"))
            ));
        }
    }

    #[test]
    fn incremental_checks_eof_at_every_offset() {
        let header = b"Content-Length: 4\r\n\r\n";
        let wire = b"Content-Length: 4\r\n\r\nnull";
        for offset in 0..=wire.len() {
            let mut decoder = IncrementalDecoder::new(FrameLimits::default());
            let mut input = &wire[..offset];
            let result = decoder.push(&mut input).unwrap();
            assert!(input.is_empty());
            assert_eq!(result.is_some(), offset == wire.len());
            if offset == 0 || offset == wire.len() {
                assert!(decoder.is_at_boundary());
                decoder.finish().unwrap();
            } else {
                assert!(!decoder.is_at_boundary());
                let expected = if offset < header.len() {
                    "EOF inside header"
                } else {
                    "EOF inside body"
                };
                assert!(
                    matches!(decoder.finish(), Err(FrameError::Invalid(message)) if message == expected)
                );
                // Checking an empty fragment or EOF does not reset partial state.
                assert!(decoder.push(&mut &[][..]).unwrap().is_none());
                assert!(!decoder.is_at_boundary());
            }
        }
        assert!(matches!(
            read_frame(&mut Cursor::new(&wire[..wire.len() - 1]), FrameLimits::default()),
            Err(FrameError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof
        ));
    }

    #[test]
    fn incremental_header_validation_matches_blocking_at_every_split() {
        let overflow = format!("Content-Length: {}0\r\n\r\n", usize::MAX);
        let malformed: &[&[u8]] = &[
            b"\r\n\r\n",
            b"Content-Length\r\n\r\n",
            b": 1\r\n\r\nx",
            b"Bad Field: 1\r\nContent-Length: 1\r\n\r\nx",
            b"Content-Length: \r\n\r\n",
            b"Content-Length: -1\r\n\r\n",
            b"Content-Length: +1\r\n\r\nx",
            b"Content-Length: 1.0\r\n\r\nx",
            b"Content-Length: 1 0\r\n\r\nx",
            b"Content-Length: 1\r\ncontent-length: 1\r\n\r\nx",
            b"Other: 2\r\n\r\n{}",
            b"Content-Length: 2\r\nX: bad\0value\r\n\r\n{}",
            b"Content-Length: 2\r\nX: bad\x7fvalue\r\n\r\n{}",
            b"Content-Length: 2\r\nX: \xff\r\n\r\n{}",
            b"Content-Length: 2\nX: a\r\n\r\n{}",
            b"Content-Length: 2\r\nContent-Type: application/json; invalid\r\n\r\n{}",
            b"Content-Length: 2\r\nContent-Type: application/json; charset=latin1\r\n\r\n{}",
            overflow.as_bytes(),
        ];
        for wire in malformed {
            let expected = read_frame(&mut Cursor::new(wire), FrameLimits::default())
                .unwrap_err()
                .to_string();
            for split in 0..=wire.len() {
                let mut decoder = IncrementalDecoder::new(FrameLimits::default());
                let result = decoder.push(&mut &wire[..split]).and_then(|frame| {
                    assert!(frame.is_none());
                    decoder.push(&mut &wire[split..])
                });
                assert_eq!(result.unwrap_err().to_string(), expected, "split {split}");
                assert_decoder_storage_is_bounded(&decoder);
            }
        }
    }

    #[test]
    fn incremental_accepts_optional_headers_without_changing_body_bytes() {
        for charset in ["utf8", "UTF-8", "\"uTf-8\""] {
            let header = format!(
                "content-length:\t004 \r\nX-Extra: value\tvalue\r\nContent-Type: application/vscode-jsonrpc; charset={charset}\r\n\r\n"
            );
            let mut wire = header.into_bytes();
            wire.extend_from_slice("🦀".as_bytes());
            let mut decoder = IncrementalDecoder::new(FrameLimits::default());
            let mut frames = Vec::new();
            for byte in wire.chunks(1) {
                decode_chunk(&mut decoder, byte, &mut frames);
            }
            assert_eq!(frames, vec!["🦀".as_bytes()]);
            decoder.finish().unwrap();
        }
    }

    #[test]
    fn incremental_rejects_limits_before_retaining_excess_bytes() {
        let wire = b"Content-Length: 4\r\n\r\nnull";
        let limits = FrameLimits {
            max_header_bytes: wire.len() - 5,
            max_content_bytes: 4,
        };
        let mut decoder = IncrementalDecoder::new(limits);
        let mut input = wire.as_slice();
        assert!(matches!(
            decoder.push(&mut input),
            Err(FrameError::HeaderTooLarge)
        ));
        assert_eq!(input, b"\nnull");
        assert_decoder_storage_is_bounded(&decoder);

        let limits = FrameLimits {
            max_header_bytes: 32,
            max_content_bytes: 3,
        };
        let mut decoder = IncrementalDecoder::new(limits);
        let mut input = wire.as_slice();
        assert!(matches!(
            decoder.push(&mut input),
            Err(FrameError::ContentTooLarge {
                actual: 4,
                limit: 3
            })
        ));
        assert_eq!(input, b"null");
        assert!(matches!(decoder.state, DecodeState::Header(_)));
        assert_decoder_storage_is_bounded(&decoder);

        let large_input = vec![b'x'; 1024 * 1024];
        let mut input = large_input.as_slice();
        let mut decoder = IncrementalDecoder::new(limits);
        assert!(matches!(
            decoder.push(&mut input),
            Err(FrameError::HeaderTooLarge)
        ));
        assert_eq!(input.len(), large_input.len() - limits.max_header_bytes);
        assert_decoder_storage_is_bounded(&decoder);

        let mut decoder = IncrementalDecoder::new(FrameLimits {
            max_header_bytes: 0,
            max_content_bytes: 0,
        });
        assert!(decoder.push(&mut &[][..]).unwrap().is_none());
        decoder.finish().unwrap();
        assert!(matches!(
            decoder.push(&mut &b"x"[..]),
            Err(FrameError::HeaderTooLarge)
        ));
        assert_decoder_storage_is_bounded(&decoder);
    }

    #[test]
    fn incremental_permissive_limits_allocate_only_for_received_bytes() {
        let limits = FrameLimits {
            max_header_bytes: usize::MAX,
            max_content_bytes: usize::MAX,
        };
        let mut decoder = IncrementalDecoder::new(limits);
        assert_decoder_storage_is_bounded(&decoder);
        let header = format!("Content-Length: {}\r\n\r\n", usize::MAX);
        assert!(decoder.push(&mut header.as_bytes()).unwrap().is_none());
        match &decoder.state {
            DecodeState::Body { length, bytes } => {
                assert_eq!(*length, usize::MAX);
                assert_eq!(bytes.capacity(), 0);
            }
            DecodeState::Header(_) => panic!("header should be complete"),
        }
        assert!(decoder.push(&mut &b"abc"[..]).unwrap().is_none());
        match &decoder.state {
            DecodeState::Body { bytes, .. } => {
                assert_eq!(bytes, b"abc");
                assert_eq!(bytes.capacity(), 3);
            }
            DecodeState::Header(_) => panic!("body should be incomplete"),
        }
        assert!(matches!(
            decoder.finish(),
            Err(FrameError::Invalid("EOF inside body"))
        ));
    }
}
