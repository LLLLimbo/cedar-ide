//! Debug Adapter Protocol envelope and framing foundation.
//!
//! DAP shares Content-Length framing with LSP, **not** JSON-RPC envelopes.
//! This module deliberately does not claim a debugger session: launch/attach,
//! breakpoints, configuration sequencing, stack/variables UI and adapter lifecycle
//! are not integrated. A future DAP process router must correlate `request_seq`
//! and route events independently, rather than using the LSP request router.

use crate::framing::{encode_json, read_frame, write_frame, FrameError, FrameLimits};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    Request {
        seq: u32,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<Value>,
    },
    Response {
        seq: u32,
        request_seq: u32,
        success: bool,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<Value>,
    },
    Event {
        seq: u32,
        event: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<Value>,
    },
}

#[derive(Debug, Error)]
pub enum DapError {
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("invalid DAP envelope: {0}")]
    Json(#[from] serde_json::Error),
}

/// Construct a minimal capability handshake. Writing this does not establish a
/// debug session; consumers must inspect the adapter's initialize response.
pub fn initialize_request(seq: u32, adapter_id: &str) -> Message {
    Message::Request {
        seq,
        command: "initialize".into(),
        arguments: Some(json!({
            "clientID":"cedar", "clientName":"Cedar", "adapterID":adapter_id,
            "pathFormat":"path", "linesStartAt1":true, "columnsStartAt1":true,
            "supportsRunInTerminalRequest":false, "supportsProgressReporting":false,
            "supportsInvalidatedEvent":false, "supportsMemoryReferences":false,
            "supportsStartDebuggingRequest":false
        })),
    }
}

pub fn read_message<R: BufRead>(
    reader: &mut R,
    limits: FrameLimits,
) -> Result<Option<Message>, DapError> {
    read_frame(reader, limits)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(DapError::from))
        .transpose()
}

pub fn write_message<W: Write>(
    writer: &mut W,
    message: &Message,
    limits: FrameLimits,
) -> Result<(), DapError> {
    let bytes = encode_json(message, limits)?;
    write_frame(writer, &bytes, limits)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn initialize_is_a_dap_request_not_json_rpc() {
        let request = initialize_request(1, "lldb");
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["type"], "request");
        assert_eq!(json["command"], "initialize");
        assert_eq!(json["arguments"]["supportsRunInTerminalRequest"], false);
        assert!(json.get("jsonrpc").is_none());
        let mut bytes = Vec::new();
        write_message(&mut bytes, &request, FrameLimits::default()).unwrap();
        assert_eq!(
            read_message(&mut Cursor::new(bytes), FrameLimits::default()).unwrap(),
            Some(request)
        );
    }

    #[test]
    fn response_and_event_roundtrip() {
        for message in [
            Message::Response {
                seq: 2,
                request_seq: 1,
                success: true,
                command: "initialize".into(),
                message: None,
                body: Some(json!({"supportsConfigurationDoneRequest":true})),
            },
            Message::Event {
                seq: 3,
                event: "initialized".into(),
                body: None,
            },
        ] {
            let mut bytes = Vec::new();
            write_message(&mut bytes, &message, FrameLimits::default()).unwrap();
            assert_eq!(
                read_message(&mut Cursor::new(bytes), FrameLimits::default()).unwrap(),
                Some(message)
            );
        }
    }
}
