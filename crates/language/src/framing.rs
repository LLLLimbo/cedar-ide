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
    let text =
        std::str::from_utf8(&header).map_err(|_| FrameError::Invalid("header must be ASCII"))?;
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
    let mut content = vec![0; length];
    reader.read_exact(&mut content)?;
    Ok(Some(content))
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
}
