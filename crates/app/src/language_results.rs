//! Bounded, inert parsing of language-server results. No returned URI is opened here.
use crate::completion::{Position, Range};
use serde_json::Value;
use std::{collections::BTreeMap, io::Write};

pub const MAX_DIAGNOSTICS: usize = 2_000;
pub const MAX_DIAGNOSTIC_FILES: usize = 128;
pub const MAX_DIAGNOSTIC_URI_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: u32,
    pub message: String,
    pub source: String,
}
#[derive(Clone, Debug)]
pub struct DiagnosticBatch {
    pub version: Option<i32>,
    pub items: Vec<Diagnostic>,
}
#[derive(Default)]
pub struct Diagnostics {
    pub files: BTreeMap<String, DiagnosticBatch>,
    pub incomplete: bool,
}
impl Diagnostics {
    pub fn clear(&mut self) {
        self.files.clear();
        self.incomplete = false;
    }
    pub fn invalidate(&mut self) {
        self.files.clear();
        self.incomplete = true;
    }
    pub fn len(&self) -> usize {
        self.files.values().map(|batch| batch.items.len()).sum()
    }
    pub fn apply(&mut self, value: &Value) -> Result<(), String> {
        let uri = bounded_string(value.get("uri"), MAX_DIAGNOSTIC_URI_BYTES)
            .ok_or("Diagnostic batch has an invalid URI")?;
        let version = match value.get("version") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or("Invalid diagnostic version")?,
            ),
        };
        if let Some(old) = self.files.get(&uri) {
            if old.version.zip(version).is_some_and(|(old, new)| new < old) {
                return Ok(());
            }
        }
        let raw = value
            .get("diagnostics")
            .and_then(Value::as_array)
            .ok_or("Missing diagnostic array")?;
        if raw.len() > MAX_DIAGNOSTICS
            || (!self.files.contains_key(&uri) && self.files.len() >= MAX_DIAGNOSTIC_FILES)
        {
            self.incomplete = true;
            return Err("Diagnostic display limit reached".into());
        }
        let existing_count = self.files.get(&uri).map_or(0, |batch| batch.items.len());
        if self.len() - existing_count + raw.len() > MAX_DIAGNOSTICS {
            self.incomplete = true;
            return Err("Diagnostic display limit reached".into());
        }
        let mut items = Vec::with_capacity(raw.len());
        for item in raw {
            items.push(Diagnostic {
                range: parse_range(item.get("range").ok_or("Diagnostic has no range")?)?,
                severity: item
                    .get("severity")
                    .and_then(Value::as_u64)
                    .filter(|s| (1..=4).contains(s))
                    .unwrap_or(3) as u32,
                message: bounded_string(item.get("message"), 16 * 1024)
                    .ok_or("Invalid diagnostic message")?,
                source: bounded_string(item.get("source"), 256).unwrap_or_default(),
            });
        }
        let retained_bytes: usize = self
            .files
            .iter()
            .filter(|(key, _)| *key != &uri)
            .map(|(key, batch)| {
                key.len()
                    + batch
                        .items
                        .iter()
                        .map(|item| item.message.len() + item.source.len())
                        .sum::<usize>()
            })
            .sum();
        let new_bytes: usize = uri.len()
            + items
                .iter()
                .map(|item| item.message.len() + item.source.len())
                .sum::<usize>();
        if retained_bytes.saturating_add(new_bytes) > 512 * 1024 {
            self.incomplete = true;
            return Err("Diagnostic text exceeds the 512 KiB display limit".into());
        }
        self.files.insert(uri, DiagnosticBatch { version, items });
        Ok(())
    }
}

pub fn parse_position(value: &Value) -> Result<Position, String> {
    Ok(Position {
        line: value
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n <= i32::MAX as u32)
            .ok_or("Invalid LSP line")?,
        character: value
            .get("character")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n <= i32::MAX as u32)
            .ok_or("Invalid LSP column")?,
    })
}
pub fn parse_range(value: &Value) -> Result<Range, String> {
    let start = parse_position(value.get("start").ok_or("Missing range start")?)?;
    let end = parse_position(value.get("end").ok_or("Missing range end")?)?;
    if (start.line, start.character) > (end.line, end.character) {
        return Err("Reversed LSP range".into());
    }
    Ok(Range { start, end })
}
pub fn parse_definitions(value: &Value) -> Result<Vec<Location>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = if let Some(values) = value.as_array() {
        values.as_slice()
    } else {
        std::slice::from_ref(value)
    };
    if values.len() > 256 {
        return Err("Definition result exceeds the 256-location limit".into());
    }
    values
        .iter()
        .map(|value| {
            let link = value.get("targetUri").is_some();
            let uri = bounded_string(value.get(if link { "targetUri" } else { "uri" }), 16 * 1024)
                .ok_or("Invalid definition URI")?;
            let range_value = if link {
                value
                    .get("targetSelectionRange")
                    .or_else(|| value.get("targetRange"))
            } else {
                value.get("range")
            };
            Ok(Location {
                uri,
                range: parse_range(range_value.ok_or("Definition has no range")?)?,
            })
        })
        .collect()
}

pub fn hover_text(value: &Value) -> String {
    fn append(value: &Value, output: &mut String) {
        if let Some(text) = value.as_str() {
            output.push_str(text);
        } else if let Some(values) = value.as_array() {
            for value in values {
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                append(value, output);
            }
        } else if let Some(text) = value.get("value").and_then(Value::as_str) {
            output.push_str(text);
        }
    }
    let mut text = String::new();
    append(value.get("contents").unwrap_or(value), &mut text);
    if text.is_empty() {
        text = "No hover information at this position".into();
    }
    truncate_text(&mut text, 64 * 1024);
    text
}
pub fn bounded_json(value: &Value, max: usize) -> String {
    struct Writer {
        bytes: Vec<u8>,
        max: usize,
        truncated: bool,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let remaining = self.max.saturating_sub(self.bytes.len());
            if bytes.len() > remaining {
                self.bytes.extend_from_slice(&bytes[..remaining]);
                self.truncated = true;
                return Err(std::io::Error::other("Display limit reached"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Vec::with_capacity(max.min(8192)),
        max,
        truncated: false,
    };
    let failed = serde_json::to_writer_pretty(&mut writer, value).is_err();
    if let Err(error) = std::str::from_utf8(&writer.bytes) {
        writer.bytes.truncate(error.valid_up_to());
    }
    let mut text = String::from_utf8(writer.bytes).unwrap_or_default();
    if failed || writer.truncated {
        text.push_str("\n[Display truncated]");
    }
    text
}

pub fn truncate_text(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[Display truncated]");
}
fn bounded_string(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| text.len() <= max)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn batch(version: Option<i32>, count: usize) -> Value {
        json!({"uri":"file:///workspace/Main.java","version":version,"diagnostics":(0..count).map(|_| json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"severity":1,"message":"problem"})).collect::<Vec<_>>()})
    }
    #[test]
    fn older_and_empty_diagnostics_are_handled_safely() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.apply(&batch(Some(3), 2)).unwrap();
        diagnostics.apply(&batch(Some(2), 0)).unwrap();
        assert_eq!(diagnostics.len(), 2);
        diagnostics.apply(&batch(Some(3), 0)).unwrap();
        assert_eq!(diagnostics.len(), 0);
    }
    #[test]
    fn unversioned_diagnostics_remain_unversioned() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.apply(&batch(None, 1)).unwrap();
        assert!(diagnostics.files.values().next().unwrap().version.is_none());
    }
    #[test]
    fn incomplete_batches_invalidate_visible_state() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.apply(&batch(Some(1), 1)).unwrap();
        diagnostics.invalidate();
        assert_eq!(diagnostics.len(), 0);
        assert!(diagnostics.incomplete);
    }
    #[test]
    fn location_links_use_selection_range_and_never_open_uris() {
        let value = json!([{"targetUri":"https://untrusted.example/","targetRange":{"start":{"line":0,"character":0},"end":{"line":8,"character":0}},"targetSelectionRange":{"start":{"line":3,"character":4},"end":{"line":3,"character":7}}}]);
        let locations = parse_definitions(&value).unwrap();
        assert_eq!(locations[0].range.start.line, 3);
        assert_eq!(locations[0].uri, "https://untrusted.example/");
        assert!(parse_definitions(&Value::Null).unwrap().is_empty());
    }
    #[test]
    fn diagnostic_positions_reject_unsigned_overflow_values() {
        let value = json!({"line":u32::MAX,"character":0});
        assert!(parse_position(&value).is_err());
    }
    #[test]
    fn json_display_is_bounded_during_serialization() {
        let mut value = json!(vec!["é🐻".repeat(1024); 5]);
        for _ in 0..100 {
            value = json!([value]);
        }
        let output = bounded_json(&value, 4096);
        assert!(output.len() <= 4096 + 32);
        assert!(output.ends_with("[Display truncated]"));
    }
    #[test]
    fn markup_is_inert_plain_text() {
        assert_eq!(
            hover_text(
                &json!({"contents":{"kind":"markdown","value":"[unsafe](https://example.com)"}})
            ),
            "[unsafe](https://example.com)"
        );
    }
}
