//! Bounded standard workspace-symbol results. URIs are data, never authority to
//! read a file; callers must resolve every navigation through the workspace.
use crate::Error;
use serde_json::{Map, Value};
use std::io::{self, Write};

const MAX_QUERY_BYTES: usize = 256;
const MAX_SYMBOLS: usize = 256;
const MAX_NAME_BYTES: usize = 4096;
const MAX_URI_BYTES: usize = 16 * 1024;
const MAX_TEXT_BYTES: usize = 512 * 1024;
// Canonical serialized Value budget, independent of the transport frame limit.
const MAX_RESULT_BYTES: usize = 1024 * 1024;
const MAX_SHAPE_DEPTH: usize = 8;
const MAX_SHAPE_NODES: usize = 16 * 1024;

pub(crate) fn validate_query(query: &str) -> Result<(), Error> {
    if query.trim().is_empty()
        || query.len() > MAX_QUERY_BYTES
        || query.chars().any(char::is_control)
    {
        return Err(Error::InvalidState(
            "Workspace symbol query must be nonblank, 1..256 UTF-8 bytes without control characters".into(),
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> Error {
    Error::Protocol(format!("Invalid workspace symbol result: {message}"))
}

struct ByteBudget(usize);
impl Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.checked_sub(bytes.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace symbol result exceeds 1 MiB",
            )
        })?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_raw_shape(value: &Value) -> Result<(), Error> {
    // Limit unknown fields too, before discarding them. Work and temporary
    // traversal storage stay bounded even for a malicious flat/deep response.
    let mut stack = vec![(value, 0)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if depth > MAX_SHAPE_DEPTH || nodes > MAX_SHAPE_NODES {
            return Err(invalid("response exceeds the shape limit"));
        }
        let children = match value {
            Value::Array(values) => values.len(),
            Value::Object(values) => values.len(),
            _ => 0,
        };
        if children + stack.len() + nodes > MAX_SHAPE_NODES {
            return Err(invalid("response exceeds the shape limit"));
        }
        match value {
            Value::Array(values) => stack.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => stack.extend(values.values().map(|value| (value, depth + 1))),
            _ => {}
        }
    }
    serde_json::to_writer(ByteBudget(MAX_RESULT_BYTES), value)
        .map_err(|_| invalid("response exceeds the 1 MiB limit"))
}

fn text(value: &Value, max_bytes: usize, nonempty: bool) -> Result<&str, Error> {
    value
        .as_str()
        .filter(|text| {
            (!nonempty || !text.is_empty())
                && text.len() <= max_bytes
                && !text.chars().any(char::is_control)
        })
        .ok_or_else(|| invalid("missing, invalid or oversized text field"))
}

fn position(value: &Value) -> Result<(u64, u64), Error> {
    let integer = |field| {
        value[field]
            .as_u64()
            .filter(|number| *number <= i32::MAX as u64)
            .ok_or_else(|| invalid("positions must be unsigned 31-bit integers"))
    };
    Ok((integer("line")?, integer("character")?))
}

fn normalized_position((line, character): (u64, u64)) -> Value {
    serde_json::json!({"line":line,"character":character})
}

pub(crate) fn normalize_result(value: Value) -> Result<Value, Error> {
    validate_raw_shape(&value)?;
    if value.is_null() {
        return Ok(Value::Null);
    }
    let symbols = value
        .as_array()
        .ok_or_else(|| invalid("expected null or a flat SymbolInformation array"))?;
    if symbols.len() > MAX_SYMBOLS {
        return Err(invalid(
            "response exceeds the 256-symbol limit; narrow the query",
        ));
    }
    let mut retained_bytes = 0usize;
    let mut normalized = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        if !symbol.is_object() || symbol.get("children").is_some() {
            return Err(invalid("expected flat SymbolInformation entries"));
        }
        let name = text(&symbol["name"], MAX_NAME_BYTES, true)?;
        let kind = symbol["kind"]
            .as_u64()
            .filter(|kind| (1..=26).contains(kind))
            .ok_or_else(|| invalid("symbol kind must be an integer in 1..=26"))?;
        let uri = text(&symbol["location"]["uri"], MAX_URI_BYTES, true)?;
        let scheme = uri.split_once(':').map(|(scheme, _)| scheme);
        if !scheme.is_some_and(|scheme| {
            scheme
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && scheme
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
        }) || uri.chars().any(char::is_whitespace)
        {
            return Err(invalid(
                "location URI must be an absolute URI without whitespace",
            ));
        }
        let range = &symbol["location"]["range"];
        if !range.is_object() {
            return Err(invalid(
                "complete location ranges are required; lazy symbol resolve is unsupported",
            ));
        }
        let start = position(&range["start"])?;
        let end = position(&range["end"])?;
        if start > end {
            return Err(invalid("location range is reversed"));
        }
        let mut entry = Map::new();
        entry.insert("name".into(), Value::String(name.into()));
        entry.insert("kind".into(), Value::from(kind));
        entry.insert(
            "location".into(),
            serde_json::json!({"uri":uri,"range":{
                "start":normalized_position(start),"end":normalized_position(end)
            }}),
        );
        retained_bytes += name.len() + uri.len();
        if let Some(container) = symbol.get("containerName") {
            let container = text(container, MAX_NAME_BYTES, false)?;
            retained_bytes += container.len();
            entry.insert("containerName".into(), Value::String(container.into()));
        }
        if let Some(tags) = symbol.get("tags") {
            let tags = tags
                .as_array()
                .filter(|tags| tags.len() <= 1 && tags.iter().all(|tag| tag.as_u64() == Some(1)))
                .ok_or_else(|| invalid("only the single Deprecated symbol tag is supported"))?;
            entry.insert("tags".into(), Value::Array(tags.clone()));
        }
        if let Some(deprecated) = symbol.get("deprecated") {
            if !deprecated.is_boolean() {
                return Err(invalid("deprecated must be boolean"));
            }
            entry.insert("deprecated".into(), deprecated.clone());
        }
        if retained_bytes > MAX_TEXT_BYTES {
            return Err(invalid(
                "retained text exceeds the 512 KiB limit; narrow the query",
            ));
        }
        // Commands, edits, data and other extensions are never retained or
        // interpreted. A malformed later entry rejects the whole response.
        normalized.push(Value::Object(entry));
    }
    Ok(Value::Array(normalized))
}
