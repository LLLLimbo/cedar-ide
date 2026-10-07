//! Completion items are untrusted edit proposals, never executable commands.
//!
//! All offsets refer to the captured source. We validate the whole proposal before
//! constructing a replacement buffer; the caller alone owns committing that buffer.
//! LSP semantics: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_completion
use serde_json::{json, Map, Value};

pub const MAX_COMPLETION_ITEMS: usize = 256;
pub const MAX_COMPLETION_EDITS: usize = 128;
pub const MAX_COMPLETION_BYTES: usize = 1024 * 1024;
const MAX_LIST_BYTES: usize = 4 * MAX_COMPLETION_BYTES;
const MAX_LABEL_BYTES: usize = 4096;
const MAX_DETAIL_BYTES: usize = 16 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_NODES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub label: String,
    pub detail: Option<String>,
    /// Normalized, bounded item, suitable for completionItem/resolve. Disabled
    /// malformed items contain null, so they cannot accidentally be applied.
    pub item: Value,
    pub disabled_reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CompletionResults {
    pub candidates: Vec<Candidate>,
    pub is_incomplete: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedCompletion {
    pub text: String,
    /// Unicode scalar offset, matching egui's character cursor convention.
    pub cursor_chars: usize,
    /// The exact JDT advisory callback was deliberately not executed.
    pub skipped_advisory: bool,
    pub edit_count: usize,
}

#[derive(Debug)]
struct Line {
    start: usize,
    end: usize,
    chars_before: usize,
}

struct TextIndex<'a> {
    source: &'a str,
    lines: Vec<Line>,
}

impl<'a> TextIndex<'a> {
    fn new(source: &'a str) -> Self {
        let mut lines = Vec::new();
        let mut chars = source.char_indices().peekable();
        let (mut start, mut chars_before, mut char_offset) = (0, 0, 0);
        while let Some((byte, c)) = chars.next() {
            char_offset += 1;
            if c == '\r' || c == '\n' {
                lines.push(Line {
                    start,
                    end: byte,
                    chars_before,
                });
                start = byte + 1;
                if c == '\r' && chars.peek().is_some_and(|(_, c)| *c == '\n') {
                    chars.next();
                    start += 1;
                    char_offset += 1;
                }
                chars_before = char_offset;
            }
        }
        // Include the empty final line, including for an empty source.
        lines.push(Line {
            start,
            end: source.len(),
            chars_before,
        });
        Self { source, lines }
    }

    fn offsets(&self, position: Position) -> Result<(usize, usize), String> {
        let line = self
            .lines
            .get(position.line as usize)
            .ok_or_else(|| "Completion position has an out-of-range line".to_owned())?;
        let (mut units, mut scalars) = (0_u32, 0_usize);
        for (relative, c) in self.source[line.start..line.end].char_indices() {
            if units == position.character {
                return Ok((line.start + relative, line.chars_before + scalars));
            }
            units = units
                .checked_add(c.len_utf16() as u32)
                .ok_or_else(|| "Completion line is too long".to_owned())?;
            scalars += 1;
            if units > position.character {
                return Err("Completion position splits a UTF-16 surrogate pair".into());
            }
        }
        if units == position.character {
            Ok((line.end, line.chars_before + scalars))
        } else {
            Err("Completion character is past the end of its line".into())
        }
    }

    fn range(&self, range: Range) -> Result<(usize, usize), String> {
        if range.start > range.end {
            return Err("Completion range is reversed".into());
        }
        Ok((self.offsets(range.start)?.0, self.offsets(range.end)?.0))
    }
}

/// Strict conversion: never clamp positions or accept the interior of CRLF or
/// a UTF-16 surrogate pair. Byte and character offsets include line terminators.
pub fn position_to_offsets(source: &str, position: Position) -> Result<(usize, usize), String> {
    TextIndex::new(source).offsets(position)
}

pub fn byte_to_position(source: &str, byte: usize) -> Result<Position, String> {
    if !source.is_char_boundary(byte) {
        return Err("Cursor is not a UTF-8 boundary".into());
    }
    let index = TextIndex::new(source);
    for (number, line) in index.lines.iter().enumerate() {
        if byte >= line.start && byte <= line.end {
            return Ok(Position {
                line: u32::try_from(number).map_err(|_| "Too many lines")?,
                character: u32::try_from(source[line.start..byte].encode_utf16().count())
                    .map_err(|_| "Cursor line is too long")?,
            });
        }
    }
    Err("Cursor is outside the document or inside a CRLF terminator".into())
}

/// Converts a scalar cursor offset without silently clamping an invalid cursor.
pub fn chars_to_position(source: &str, chars: usize) -> Result<Position, String> {
    let byte = source
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(source.len()))
        .nth(chars)
        .ok_or_else(|| "Cursor is outside the document".to_owned())?;
    byte_to_position(source, byte)
}

pub fn range_to_offsets(source: &str, range: Range) -> Result<(usize, usize), String> {
    TextIndex::new(source).range(range)
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{name} must be an object"))
}

fn known_fields(map: &Map<String, Value>, allowed: &[&str], name: &str) -> Result<(), String> {
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("Unsupported {name} field: {}", clipped(key, 80)));
    }
    Ok(())
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    map.get(key)
        .ok_or_else(|| format!("Completion is missing {key}"))
}

fn string<'a>(value: &'a Value, name: &str, limit: usize) -> Result<&'a str, String> {
    let s = value
        .as_str()
        .ok_or_else(|| format!("{name} must be a string"))?;
    if s.len() > limit {
        return Err(format!("{name} exceeds the {limit}-byte limit"));
    }
    Ok(s)
}

fn parse_position(value: &Value) -> Result<Position, String> {
    let map = object(value, "Position")?;
    known_fields(map, &["line", "character"], "position")?;
    let integer = |key| -> Result<u32, String> {
        required(map, key)?
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("Position {key} must be a nonnegative 32-bit integer"))
    };
    Ok(Position {
        line: integer("line")?,
        character: integer("character")?,
    })
}

fn parse_range(value: &Value) -> Result<Range, String> {
    let map = object(value, "Range")?;
    known_fields(map, &["start", "end"], "range")?;
    let range = Range {
        start: parse_position(required(map, "start")?)?,
        end: parse_position(required(map, "end")?)?,
    };
    if range.start > range.end {
        return Err("Completion range is reversed".into());
    }
    Ok(range)
}

// Bound opaque data/documentation before cloning. This does not interpret the
// server's data, and does not serialize huge values just to measure their size.
fn json_cost(value: &Value) -> Result<usize, String> {
    fn visit(
        value: &Value,
        depth: usize,
        bytes: &mut usize,
        nodes: &mut usize,
    ) -> Result<(), String> {
        *nodes += 1;
        if depth > MAX_JSON_DEPTH || *nodes > MAX_JSON_NODES {
            return Err("Completion data is too complex".into());
        }
        *bytes = bytes.saturating_add(8);
        match value {
            Value::String(s) => *bytes = bytes.saturating_add(s.len()),
            Value::Array(values) => {
                for child in values {
                    visit(child, depth + 1, bytes, nodes)?;
                }
            }
            Value::Object(map) => {
                for (key, child) in map {
                    *bytes = bytes.saturating_add(key.len());
                    visit(child, depth + 1, bytes, nodes)?;
                }
            }
            _ => {}
        }
        if *bytes > MAX_COMPLETION_BYTES {
            return Err("Completion data exceeds the 1 MiB limit".into());
        }
        Ok(())
    }
    let (mut bytes, mut nodes) = (0, 0);
    visit(value, 0, &mut bytes, &mut nodes)?;
    Ok(bytes)
}

fn clipped(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn normalize_defaults(item: &Value, defaults: Option<&Value>) -> Result<Value, String> {
    let mut map = object(item, "Completion item")?.clone();
    if let Some(defaults) = defaults {
        let defaults = object(defaults, "Completion itemDefaults")?;
        known_fields(
            defaults,
            &[
                "commitCharacters",
                "editRange",
                "insertTextFormat",
                "insertTextMode",
                "data",
            ],
            "itemDefaults",
        )?;
        for key in [
            "commitCharacters",
            "insertTextFormat",
            "insertTextMode",
            "data",
        ] {
            if !map.contains_key(key) {
                if let Some(value) = defaults.get(key) {
                    map.insert(key.into(), value.clone());
                }
            }
        }
        if !map.contains_key("textEdit") {
            if let Some(range) = defaults.get("editRange") {
                let new_text = map
                    .get("textEditText")
                    .or_else(|| map.get("label"))
                    .ok_or_else(|| "Completion is missing its label".to_owned())?
                    .clone();
                let range_map = object(range, "Default edit range")?;
                let edit = if range_map.contains_key("start") || range_map.contains_key("end") {
                    parse_range(range)?;
                    json!({"range": range, "newText": new_text})
                } else {
                    known_fields(range_map, &["insert", "replace"], "default edit range")?;
                    let insert = required(range_map, "insert")?;
                    let replace = required(range_map, "replace")?;
                    validate_insert_replace(parse_range(insert)?, parse_range(replace)?)?;
                    json!({"insert": insert, "replace": replace, "newText": new_text})
                };
                map.insert("textEdit".into(), edit);
            }
        }
    }
    Ok(Value::Object(map))
}

fn validate_insert_replace(insert: Range, replace: Range) -> Result<(), String> {
    if insert.start.line != insert.end.line || replace.start.line != replace.end.line {
        return Err("Completion primary edits must stay on one source line".into());
    }
    if insert.start != replace.start || insert.end > replace.end {
        return Err("Completion insert range must be a prefix of its replace range".into());
    }
    Ok(())
}

fn validate_edit(value: &Value, primary: bool) -> Result<(Range, Option<Range>, &str), String> {
    let edit = object(value, "Text edit")?;
    let new_text = string(
        required(edit, "newText")?,
        "Edit text",
        MAX_COMPLETION_BYTES,
    )?;
    if edit.contains_key("range") {
        known_fields(edit, &["range", "newText"], "text edit")?;
        let range = parse_range(required(edit, "range")?)?;
        if primary && range.start.line != range.end.line {
            return Err("Completion primary edits must stay on one source line".into());
        }
        Ok((range, None, new_text))
    } else if primary {
        known_fields(
            edit,
            &["insert", "replace", "newText"],
            "insert/replace edit",
        )?;
        let insert = parse_range(required(edit, "insert")?)?;
        let replace = parse_range(required(edit, "replace")?)?;
        validate_insert_replace(insert, replace)?;
        // The UI deliberately accepts in replace mode, not insert mode.
        Ok((replace, Some(insert), new_text))
    } else {
        Err("Additional edits must be same-document plain TextEdits".into())
    }
}

/// Structural validation is used before rendering candidates and again after
/// resolve. In particular, resolve cannot smuggle in commands or other edits.
fn validate_item(item: &Value) -> Result<bool, String> {
    json_cost(item)?;
    let map = object(item, "Completion item")?;
    known_fields(
        map,
        &[
            "label",
            "labelDetails",
            "kind",
            "tags",
            "detail",
            "documentation",
            "deprecated",
            "preselect",
            "sortText",
            "filterText",
            "insertText",
            "insertTextFormat",
            "insertTextMode",
            "textEdit",
            "textEditText",
            "additionalTextEdits",
            "commitCharacters",
            "command",
            "data",
        ],
        "completion item",
    )?;
    string(required(map, "label")?, "Completion label", MAX_LABEL_BYTES)?;
    for key in ["detail", "sortText", "filterText"] {
        if let Some(value) = map.get(key) {
            string(value, key, MAX_DETAIL_BYTES)?;
        }
    }
    for key in ["insertText", "textEditText"] {
        if let Some(value) = map.get(key) {
            string(value, key, MAX_COMPLETION_BYTES)?;
        }
    }
    if map.contains_key("textEditText") && !map.contains_key("textEdit") {
        return Err("textEditText requires a supported default edit range".into());
    }
    for key in ["deprecated", "preselect"] {
        if map.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(format!("{key} must be a boolean"));
        }
    }
    if map.get("kind").is_some_and(|v| v.as_u64().is_none()) {
        return Err("Completion kind must be a nonnegative integer".into());
    }
    if let Some(tags) = map.get("tags") {
        if !tags
            .as_array()
            .is_some_and(|tags| tags.iter().all(|v| v.as_u64().is_some()))
        {
            return Err("Completion tags must be integers".into());
        }
    }
    if let Some(details) = map.get("labelDetails") {
        let details = object(details, "Label details")?;
        known_fields(details, &["detail", "description"], "label details")?;
        for (key, value) in details {
            string(value, key, MAX_DETAIL_BYTES)?;
        }
    }
    if let Some(documentation) = map.get("documentation") {
        if !documentation.is_string() {
            let markup = object(documentation, "Documentation")?;
            known_fields(markup, &["kind", "value"], "documentation")?;
            let kind = string(required(markup, "kind")?, "Documentation kind", 32)?;
            if kind != "plaintext" && kind != "markdown" {
                return Err("Unsupported documentation format".into());
            }
            string(
                required(markup, "value")?,
                "Documentation",
                MAX_COMPLETION_BYTES,
            )?;
        }
    }
    if let Some(format) = map.get("insertTextFormat") {
        match format.as_u64() {
            Some(1) => {}
            Some(2) => return Err("Snippet completions are not supported".into()),
            _ => return Err("Unsupported completion insertTextFormat".into()),
        }
    }
    if let Some(mode) = map.get("insertTextMode") {
        match mode.as_u64() {
            Some(1) => {}
            Some(2) => {
                // Real JDT LS uses adjustIndentation even for one-line text.
                // It has no effect there. Only the primary insertion uses this
                // mode; import edits remain exact plain TextEdits, even when
                // their newText contains newlines.
                let primary_text = if let Some(edit) = map.get("textEdit") {
                    validate_edit(edit, true)?.2
                } else {
                    string(
                        map.get("insertText").unwrap_or(required(map, "label")?),
                        "Insert text",
                        MAX_COMPLETION_BYTES,
                    )?
                };
                if primary_text.contains('\r') || primary_text.contains('\n') {
                    return Err(
                        "Multiline completion indentation adjustment is not supported".into(),
                    );
                }
            }
            _ => return Err("Unsupported completion insertTextMode".into()),
        }
    }
    if let Some(characters) = map.get("commitCharacters") {
        let characters = characters
            .as_array()
            .ok_or("commitCharacters must be an array")?;
        if characters.len() > 256
            || characters
                .iter()
                .any(|v| !v.as_str().is_some_and(|s| s.chars().count() == 1))
        {
            return Err("Unsupported completion commitCharacters".into());
        }
        // Enter/click accepts explicitly. Commit characters never auto-apply.
    }
    if let Some(edit) = map.get("textEdit") {
        validate_edit(edit, true)?;
    }
    if let Some(edits) = map.get("additionalTextEdits") {
        let edits = edits
            .as_array()
            .ok_or("additionalTextEdits must be an array")?;
        if edits.len() >= MAX_COMPLETION_EDITS {
            return Err("Completion exceeds the 128-edit limit".into());
        }
        for edit in edits {
            validate_edit(edit, false)?;
        }
    }
    if let Some(command) = map.get("command") {
        let command = object(command, "Completion command")?;
        known_fields(
            command,
            &["title", "command", "arguments"],
            "completion command",
        )?;
        string(
            required(command, "title")?,
            "Command title",
            MAX_DETAIL_BYTES,
        )?;
        let name = string(
            required(command, "command")?,
            "Command name",
            MAX_LABEL_BYTES,
        )?;
        if name != "java.completion.onDidSelect" {
            return Err("Completion requires an unsupported command; no edits were applied".into());
        }
        if command
            .get("arguments")
            .is_some_and(|args| !args.is_array())
        {
            return Err("Completion command arguments must be an array".into());
        }
        // Exact Eclipse JDT callback: signature-help/cache/ranking advisory.
        // It is never run. Imports must arrive as additionalTextEdits instead.
        return Ok(true);
    }
    Ok(false)
}

/// Accept an array, CompletionList, or null. Unsupported rows remain visible
/// with a reason, but contain no actionable payload.
pub fn parse_completion_result(value: &Value) -> Result<CompletionResults, String> {
    if value.is_null() {
        return Ok(CompletionResults::default());
    }
    let (items, defaults, is_incomplete) = if let Some(items) = value.as_array() {
        (items, None, false)
    } else {
        let list = object(value, "Completion result")?;
        known_fields(
            list,
            &["items", "isIncomplete", "itemDefaults"],
            "completion list",
        )?;
        let items = required(list, "items")?
            .as_array()
            .ok_or("Completion items must be an array")?;
        let incomplete = required(list, "isIncomplete")?
            .as_bool()
            .ok_or("isIncomplete must be a boolean")?;
        (items, list.get("itemDefaults"), incomplete)
    };
    let default_error = defaults.and_then(|defaults| json_cost(defaults).err());
    let mut result = CompletionResults {
        candidates: Vec::new(),
        is_incomplete,
        truncated: items.len() > MAX_COMPLETION_ITEMS,
    };
    let mut total_bytes = 0_usize;
    for raw in items.iter().take(MAX_COMPLETION_ITEMS) {
        let label = raw
            .get("label")
            .and_then(Value::as_str)
            .map(|s| clipped(s, MAX_LABEL_BYTES))
            .unwrap_or_else(|| "<invalid completion>".into());
        let detail = raw
            .get("detail")
            .and_then(Value::as_str)
            .map(|s| clipped(s, MAX_DETAIL_BYTES));
        let normalized = (|| -> Result<Value, String> {
            if let Some(error) = &default_error {
                return Err(error.clone());
            }
            json_cost(raw)?;
            let item = normalize_defaults(raw, defaults)?;
            let cost = json_cost(&item)?;
            if total_bytes.saturating_add(cost) > MAX_LIST_BYTES {
                result.truncated = true;
                return Err("Completion list exceeds the 4 MiB payload limit".into());
            }
            total_bytes += cost;
            validate_item(&item)?;
            Ok(item)
        })();
        let (item, disabled_reason) = match normalized {
            Ok(item) => (item, None),
            Err(error) => (Value::Null, Some(error)),
        };
        result.candidates.push(Candidate {
            label,
            detail,
            item,
            disabled_reason,
        });
    }
    Ok(result)
}

struct Edit<'a> {
    start: usize,
    end: usize,
    text: &'a str,
    primary: bool,
}

fn identifier(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

fn edits_overlap(a: &Edit<'_>, b: &Edit<'_>) -> bool {
    if a.start == a.end {
        return a.start >= b.start && a.start <= b.end;
    }
    if b.start == b.end {
        return b.start >= a.start && b.start <= a.end;
    }
    a.start < b.end && b.start < a.end
}

/// Validate and apply in memory as one transaction. Callers must additionally
/// verify that their current document/path/version still matches this snapshot.
/// No command, workspace edit, disk write, or cross-document edit is performed.
pub fn apply_completion(
    source: &str,
    cursor: Position,
    item: &Value,
) -> Result<AppliedCompletion, String> {
    if source.len() > MAX_COMPLETION_BYTES {
        return Err("Completion source exceeds the 1 MiB limit".into());
    }
    let skipped_advisory = validate_item(item)?;
    let map = object(item, "Completion item")?;
    let index = TextIndex::new(source);
    let (cursor_byte, _) = index.offsets(cursor)?;
    let primary = if let Some(value) = map.get("textEdit") {
        let (replace, insert, text) = validate_edit(value, true)?;
        let (start, end) = index.range(replace)?;
        let acceptance = insert.unwrap_or(replace);
        index.range(acceptance)?;
        if cursor < acceptance.start || cursor > acceptance.end {
            return Err("Completion primary range does not contain the captured cursor".into());
        }
        Edit {
            start,
            end,
            text,
            primary: true,
        }
    } else {
        let text = string(
            map.get("insertText").unwrap_or(required(map, "label")?),
            "Insert text",
            MAX_COMPLETION_BYTES,
        )?;
        let mut start = cursor_byte;
        for (byte, c) in source[..cursor_byte].char_indices().rev() {
            if !identifier(c) {
                break;
            }
            start = byte;
        }
        Edit {
            start,
            end: cursor_byte,
            text,
            primary: true,
        }
    };
    let mut edits = vec![primary];
    if let Some(additional) = map.get("additionalTextEdits").and_then(Value::as_array) {
        for value in additional {
            let (range, _, text) = validate_edit(value, false)?;
            let (start, end) = index.range(range)?;
            edits.push(Edit {
                start,
                end,
                text,
                primary: false,
            });
        }
    }
    if edits.len() > MAX_COMPLETION_EDITS {
        return Err("Completion exceeds the 128-edit limit".into());
    }
    let mut inserted = 0_usize;
    let mut removed = 0_usize;
    for (i, edit) in edits.iter().enumerate() {
        inserted = inserted
            .checked_add(edit.text.len())
            .ok_or("Completion edit size overflow")?;
        removed = removed
            .checked_add(edit.end - edit.start)
            .ok_or("Completion edit size overflow")?;
        if inserted > MAX_COMPLETION_BYTES {
            return Err("Completion edit text exceeds the 1 MiB limit".into());
        }
        if edits[..i].iter().any(|other| edits_overlap(edit, other)) {
            return Err("Completion edits overlap or share an ambiguous insertion boundary".into());
        }
    }
    let final_size = source
        .len()
        .checked_sub(removed)
        .and_then(|n| n.checked_add(inserted))
        .ok_or("Completion result size overflow")?;
    if final_size > MAX_COMPLETION_BYTES {
        return Err("Completed document exceeds the 1 MiB limit".into());
    }
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut text = String::with_capacity(final_size);
    let (mut previous, mut cursor_after) = (0, 0);
    for edit in &edits {
        text.push_str(&source[previous..edit.start]);
        text.push_str(edit.text);
        if edit.primary {
            cursor_after = text.len();
        }
        previous = edit.end;
    }
    text.push_str(&source[previous..]);
    let cursor_chars = text[..cursor_after].chars().count();
    Ok(AppliedCompletion {
        text,
        cursor_chars,
        skipped_advisory,
        edit_count: edits.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: u32, character: u32) -> Position {
        Position { line, character }
    }
    fn r(line: u32, start: u32, end: u32) -> Value {
        json!({"start":{"line":line,"character":start},"end":{"line":line,"character":end}})
    }
    fn edit(line: u32, start: u32, end: u32, text: &str) -> Value {
        json!({"range":r(line,start,end),"newText":text})
    }
    fn item(text_edit: Value) -> Value {
        json!({"label":"result","textEdit":text_edit})
    }

    #[test]
    fn dense_unicode_offsets_are_utf16_and_scalar_correct() {
        let s = "α😀中𐐀e\u{301}\nδ";
        for (units, byte, chars) in [
            (0, 0, 0),
            (1, 2, 1),
            (3, 6, 2),
            (4, 9, 3),
            (6, 13, 4),
            (7, 14, 5),
            (8, 16, 6),
        ] {
            assert_eq!(position_to_offsets(s, p(0, units)).unwrap(), (byte, chars));
            assert_eq!(byte_to_position(s, byte).unwrap(), p(0, units));
        }
        assert!(position_to_offsets(s, p(0, 2)).is_err());
        assert!(position_to_offsets(s, p(0, 5)).is_err());
        assert!(position_to_offsets(s, p(0, 9)).is_err());
        assert_eq!(position_to_offsets(s, p(1, 1)).unwrap(), (19, 8));
        assert!(byte_to_position(s, 4).is_err());
    }

    #[test]
    fn crlf_and_lf_endpoints_are_strict() {
        let s = "a😀\r\n中\n\r\n";
        assert_eq!(position_to_offsets(s, p(0, 3)).unwrap(), (5, 2));
        assert_eq!(position_to_offsets(s, p(1, 0)).unwrap(), (7, 4));
        assert_eq!(position_to_offsets(s, p(2, 0)).unwrap(), (11, 6));
        assert_eq!(position_to_offsets(s, p(3, 0)).unwrap(), (13, 8));
        assert!(position_to_offsets(s, p(0, 4)).is_err());
        assert!(byte_to_position(s, 6).is_err());
        assert!(byte_to_position(s, 12).is_err());
        assert!(position_to_offsets(s, p(4, 0)).is_err());
        assert_eq!(position_to_offsets("", p(0, 0)).unwrap(), (0, 0));
        assert!(position_to_offsets("", p(0, 1)).is_err());
        assert_eq!(position_to_offsets("x\ry", p(1, 0)).unwrap(), (2, 2));
    }

    #[test]
    fn scalar_cursor_conversion_rejects_crlf_interior() {
        assert_eq!(chars_to_position("😀\r\nx", 3).unwrap(), p(1, 0));
        assert!(chars_to_position("😀\r\nx", 2).is_err());
        assert!(chars_to_position("😀", 2).is_err());
        assert_eq!(chars_to_position("😀", 1).unwrap(), p(0, 2));
    }

    #[test]
    fn malformed_and_reversed_ranges_cannot_mutate() {
        let source = "😀abc\r\nx";
        let mut bad = vec![
            edit(0, 3, 2, "z"),
            edit(0, 1, 3, "z"),
            edit(0, 0, 99, "z"),
            edit(4, 0, 0, "z"),
        ];
        bad.push(json!({"range":{"start":{"line":-1,"character":0},"end":{"line":0,"character":2}},"newText":"z"}));
        bad.push(json!({"range":{"start":{"line":0.1,"character":0},"end":{"line":0,"character":2}},"newText":"z"}));
        bad.push(json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":4294967296_u64}},"newText":"z"}));
        bad.push(json!({"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}},"newText":"z"}));
        for value in bad {
            assert!(apply_completion(source, p(0, 2), &item(value)).is_err());
        }
        assert!(apply_completion("abcdef", p(0, 1), &item(edit(0, 3, 5, "x"))).is_err());
        assert_eq!(source, "😀abc\r\nx");
    }

    #[test]
    fn imports_and_primary_edit_are_atomic_with_unicode_cursor() {
        let source = "package demo;\r\n// 😀\r\nclass Demo { Arr suffix; }\r\n";
        let value = json!({"label":"ArrayList", "textEdit":edit(2,13,16,"ArrayList<中>"),
            "additionalTextEdits":[edit(1,0,0,"import java.util.ArrayList;\r\n")]});
        let applied = apply_completion(source, p(2, 16), &value).unwrap();
        assert_eq!(applied.text,"package demo;\r\nimport java.util.ArrayList;\r\n// 😀\r\nclass Demo { ArrayList<中> suffix; }\r\n");
        assert_eq!(
            applied.cursor_chars,
            applied.text[..applied.text.find(" suffix").unwrap()]
                .chars()
                .count()
        );
        assert_eq!(applied.edit_count, 2);
        assert!(!applied.skipped_advisory);
    }

    #[test]
    fn primary_cursor_accounts_for_changes_before_but_not_after() {
        let value = json!({"label":"🌳", "textEdit":edit(1,0,1,"🌳"),
            "additionalTextEdits":[edit(0,0,3,"中"),edit(2,0,1,"longer")]});
        let applied = apply_completion("abc\nx\ny", p(1, 1), &value).unwrap();
        assert_eq!(applied.text, "中\n🌳\nlonger");
        assert_eq!(applied.cursor_chars, 3);
    }

    #[test]
    fn same_point_and_nested_overlaps_reject_whole_plan() {
        for secondary in [
            edit(0, 1, 1, "x"),
            edit(0, 0, 0, "x"),
            edit(0, 3, 3, "x"),
            edit(0, 2, 4, "x"),
            edit(0, 0, 3, "x"),
        ] {
            let value = json!({"label":"foo","textEdit":edit(0,0,3,"foo"),"additionalTextEdits":[secondary]});
            assert!(apply_completion("abcde", p(0, 3), &value)
                .unwrap_err()
                .contains("overlap"));
        }
        let same =
            json!({"label":"x","textEdit":edit(0,2,2,"x"),"additionalTextEdits":[edit(0,2,2,"y")]});
        assert!(apply_completion("abcd", p(0, 2), &same).is_err());
        let imports = json!({"label":"x","textEdit":edit(1,0,1,"x"),"additionalTextEdits":[edit(0,0,0,"a"),edit(0,0,0,"b")]});
        assert!(apply_completion("\nz", p(1, 1), &imports).is_err());
    }

    #[test]
    fn adjacent_nonempty_edits_are_unambiguous() {
        let value =
            json!({"label":"x","textEdit":edit(0,1,2,"x"),"additionalTextEdits":[edit(0,0,1,"y")]});
        let applied = apply_completion("ab", p(0, 2), &value).unwrap();
        assert_eq!(applied.text, "yx");
        assert_eq!(applied.cursor_chars, 2);
    }

    #[test]
    fn invalid_additional_edit_never_partially_applies() {
        let source = "Arr";
        let value = json!({"label":"ArrayList","textEdit":edit(0,0,3,"ArrayList"),"additionalTextEdits":[edit(99,0,0,"import!")]});
        assert!(apply_completion(source, p(0, 3), &value).is_err());
        assert_eq!(source, "Arr");
    }

    #[test]
    fn insert_replace_deliberately_replaces_suffix() {
        let value = item(json!({"insert":r(0,0,3),"replace":r(0,0,7),"newText":"ArrayList"}));
        let applied = apply_completion("ArrList end", p(0, 3), &value).unwrap();
        assert_eq!(applied.text, "ArrayList end");
        assert_eq!(applied.cursor_chars, 9);
        assert!(apply_completion("ArrList", p(0, 4), &value).is_err());
        for (insert, replace) in [
            (r(0, 1, 3), r(0, 0, 7)),
            (r(0, 0, 7), r(0, 0, 3)),
            (r(0, 0, 2), r(0, 0, 99)),
        ] {
            assert!(apply_completion(
                "ArrList",
                p(0, 2),
                &item(json!({"insert":insert,"replace":replace,"newText":"x"}))
            )
            .is_err());
        }
    }

    #[test]
    fn absent_edit_replaces_only_captured_identifier_prefix() {
        let applied = apply_completion(
            "obj.中_δ$2Tail",
            p(0, 9),
            &json!({"label":"wrong","insertText":"选择"}),
        )
        .unwrap();
        assert_eq!(applied.text, "obj.选择Tail");
        assert_eq!(applied.cursor_chars, 6);
        let applied = apply_completion("obj.𐐀_", p(0, 7), &json!({"label":"name"})).unwrap();
        assert_eq!(applied.text, "obj.name");
        assert_eq!(
            apply_completion("", p(0, 0), &json!({"label":"hello"}))
                .unwrap()
                .text,
            "hello"
        );
    }

    #[test]
    fn unsupported_snippets_commands_and_complex_edits_are_disabled() {
        for extra in [
            json!({"insertTextFormat":2}),
            json!({"insertTextMode":2,"insertText":"first\nsecond"}),
            json!({"command":{"title":"Import","command":"java.apply.workspaceEdit","arguments":[]}}),
            json!({"command":{"title":"x","command":"java.completion.onDidSelect.extra"}}),
            json!({"workspaceEdit":{"changes":{}}}),
            json!({"additionalTextEdits":{}}),
            json!({"textEdit":{"range":r(0,0,1),"newText":"x","uri":"file:///another"}}),
            json!({"additionalTextEdits":[{"insert":r(0,0,0),"replace":r(0,0,0),"newText":"x"}]}),
        ] {
            let mut value = json!({"label":"x"});
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let result = parse_completion_result(&json!([value.clone()])).unwrap();
            assert!(result.candidates[0].disabled_reason.is_some(), "{value}");
            assert!(result.candidates[0].item.is_null());
            assert!(apply_completion("x", p(0, 1), &value).is_err());
        }
    }

    #[test]
    fn real_jdt_resolve_skips_exact_advisory_but_keeps_imports() {
        // Full captured JDT LS 1.61 response, not an abbreviated synthetic item.
        let evidence =
            include_str!("../../language/tests/evidence/jdtls-1.61.0-completion-resolve.jsonl");
        let record = evidence
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|record| record["kind"] == "completion_resolve")
            .unwrap();
        let input = &record["input"];
        let value = &record["payload"];
        assert_eq!(value["insertTextMode"], 2);
        assert_eq!(value["insertTextFormat"], 1);
        assert_eq!(input["data"], json!({"pid":"1","rid":"0"}));
        assert!(
            parse_completion_result(&json!([input])).unwrap().candidates[0]
                .disabled_reason
                .is_none()
        );
        let source = "public class Main {\n    public static void main(String[] args) {\n        String greeting = \"Hello Cedar\";\n        System.out.println(greeting);\n        int broken = \"oops\";\n    }\n}\n";
        let applied = apply_completion(source, p(3, 30), value).unwrap();
        assert_eq!(
            applied.text,
            format!(
                "import java.util.GregorianCalendar;\n\n{}",
                source.replace("println(greeting)", "println(GregorianCalendar)")
            )
        );
        assert!(applied.skipped_advisory);
        assert_eq!(applied.edit_count, 2);
        assert_eq!(
            applied.cursor_chars,
            applied.text.find("GregorianCalendar);").unwrap() + "GregorianCalendar".len()
        );
        assert!(
            parse_completion_result(&json!([value])).unwrap().candidates[0]
                .disabled_reason
                .is_none()
        );
    }

    #[test]
    fn adjust_indentation_accepts_only_single_line_primary_text() {
        for newline in ["\n", "\r\n", "\r"] {
            let value = json!({"label":"x","insertTextMode":2,"textEdit":edit(0,0,1,&format!("x{newline}y"))});
            assert!(apply_completion("x", p(0, 1), &value)
                .unwrap_err()
                .contains("indentation"));
        }
        let direct = json!({"label":"result","insertTextMode":2});
        assert_eq!(
            apply_completion("res", p(0, 3), &direct).unwrap().text,
            "result"
        );
        let explicit = json!({"label":"multiline\nlabel","insertText":"ignored\ntext",
            "insertTextMode":2,"textEdit":edit(0,0,1,"singleLine")});
        assert_eq!(
            apply_completion("x", p(0, 1), &explicit).unwrap().text,
            "singleLine"
        );
        for mode in [json!(0), json!(3), json!("2"), Value::Null] {
            assert!(
                apply_completion("x", p(0, 1), &json!({"label":"y","insertTextMode":mode}))
                    .is_err()
            );
        }
    }

    #[test]
    fn list_defaults_are_expanded_before_resolution() {
        let raw = json!({"isIncomplete":true,"itemDefaults":{"editRange":r(0,0,3),"insertTextFormat":1,"insertTextMode":1,"data":{"request":42},"commitCharacters":[".",";"]},
            "items":[{"label":"ArrayList","insertText":"ignored"},{"label":"other","textEditText":"Chosen","data":{"override":true}}]});
        let parsed = parse_completion_result(&raw).unwrap();
        assert!(parsed.is_incomplete);
        assert!(!parsed.truncated);
        assert_eq!(parsed.candidates[0].item["data"]["request"], 42);
        assert_eq!(
            apply_completion("Arr", p(0, 3), &parsed.candidates[0].item)
                .unwrap()
                .text,
            "ArrayList"
        );
        assert_eq!(
            apply_completion("Arr", p(0, 3), &parsed.candidates[1].item)
                .unwrap()
                .text,
            "Chosen"
        );
        assert_eq!(parsed.candidates[1].item["data"], json!({"override":true}));
    }

    #[test]
    fn default_insert_replace_and_explicit_overrides() {
        let raw = json!({"isIncomplete":false,"itemDefaults":{"editRange":{"insert":r(0,0,2),"replace":r(0,0,4)},"insertTextFormat":2},
            "items":[{"label":"Name","insertTextFormat":1},{"label":"Snippet"},{"label":"Explicit","insertTextFormat":1,"textEdit":edit(0,0,2,"X")}]});
        let parsed = parse_completion_result(&raw).unwrap();
        assert_eq!(
            apply_completion("Naaa", p(0, 2), &parsed.candidates[0].item)
                .unwrap()
                .text,
            "Name"
        );
        assert!(parsed.candidates[1]
            .disabled_reason
            .as_ref()
            .unwrap()
            .contains("Snippet"));
        assert_eq!(
            apply_completion("Naaa", p(0, 2), &parsed.candidates[2].item)
                .unwrap()
                .text,
            "Xaa"
        );
    }

    #[test]
    fn unknown_defaults_and_malformed_results_are_explicit() {
        let raw = json!({"isIncomplete":false,"itemDefaults":{"unknownEditMode":{}},"items":[{"label":"x"}]});
        let parsed = parse_completion_result(&raw).unwrap();
        assert!(parsed.candidates[0]
            .disabled_reason
            .as_ref()
            .unwrap()
            .contains("itemDefaults"));
        assert!(parse_completion_result(&json!({"items":[]})).is_err());
        assert!(parse_completion_result(
            &json!({"isIncomplete":false,"items":{},"itemDefaults":{}})
        )
        .is_err());
        assert!(parse_completion_result(&json!([null,1,{"label":false}]))
            .unwrap()
            .candidates
            .iter()
            .all(|c| c.disabled_reason.is_some()));
        assert!(parse_completion_result(&Value::Null)
            .unwrap()
            .candidates
            .is_empty());
    }

    #[test]
    fn bounded_count_strings_documents_and_edit_payloads() {
        let raw = Value::Array(vec![json!({"label":"x"}); MAX_COMPLETION_ITEMS + 1]);
        let parsed = parse_completion_result(&raw).unwrap();
        assert_eq!(parsed.candidates.len(), MAX_COMPLETION_ITEMS);
        assert!(parsed.truncated);
        let large = "a".repeat(MAX_COMPLETION_BYTES + 1);
        assert!(apply_completion(&large, p(0, 0), &json!({"label":"x"})).is_err());
        assert!(apply_completion("", p(0, 0), &json!({"label":"x","insertText":large})).is_err());
        let label = "中".repeat(MAX_LABEL_BYTES);
        let parsed = parse_completion_result(&json!([{"label":label}])).unwrap();
        assert!(parsed.candidates[0].label.len() <= MAX_LABEL_BYTES + 3);
        assert!(parsed.candidates[0].disabled_reason.is_some());
        let value =
            json!({"label":"x","additionalTextEdits":vec![edit(0,0,0,"");MAX_COMPLETION_EDITS]});
        assert!(apply_completion("", p(0, 0), &value)
            .unwrap_err()
            .contains("128-edit"));
        let source = format!("{} ", "a".repeat(MAX_COMPLETION_BYTES - 2));
        assert!(apply_completion(
            &source,
            p(0, MAX_COMPLETION_BYTES as u32 - 1),
            &json!({"label":"overflow"})
        )
        .unwrap_err()
        .contains("1 MiB"));
    }

    #[test]
    fn combined_list_payload_is_bounded() {
        let item = json!({"label":"x","documentation":"d".repeat(MAX_COMPLETION_BYTES/2)});
        let parsed = parse_completion_result(&Value::Array(vec![item; 16])).unwrap();
        assert!(parsed.truncated);
        assert!(
            parsed
                .candidates
                .iter()
                .filter(|c| c.disabled_reason.is_none())
                .count()
                < 8
        );
    }

    #[test]
    fn exactly_128_edits_work_regardless_of_server_order() {
        let source = "x\n".repeat(MAX_COMPLETION_EDITS);
        let additional: Vec<_> = (0..MAX_COMPLETION_EDITS - 1)
            .rev()
            .map(|line| edit(line as u32, 0, 1, "中"))
            .collect();
        let value = json!({"label":"😀","textEdit":edit(127,0,1,"😀"),
            "additionalTextEdits":additional});
        let applied = apply_completion(&source, p(127, 1), &value).unwrap();
        assert_eq!(applied.edit_count, MAX_COMPLETION_EDITS);
        assert_eq!(applied.text, format!("{}😀\n", "中\n".repeat(127)));
        assert_eq!(applied.cursor_chars, 255);
    }

    #[test]
    fn every_representable_unicode_boundary_round_trips() {
        let source = "\r\na😀中\r\n𐐀_δ$\ne\u{301}\r";
        for byte in (0..=source.len()).filter(|byte| source.is_char_boundary(*byte)) {
            let in_crlf = byte > 0
                && source.as_bytes()[byte - 1] == b'\r'
                && source.as_bytes().get(byte) == Some(&b'\n');
            let position = byte_to_position(source, byte);
            if in_crlf {
                assert!(position.is_err());
            } else {
                let (actual, chars) = position_to_offsets(source, position.unwrap()).unwrap();
                assert_eq!(actual, byte);
                assert_eq!(chars, source[..byte].chars().count());
            }
        }
    }

    #[test]
    fn deeply_nested_opaque_data_is_rejected_without_cloning() {
        let mut nested = Value::Null;
        for _ in 0..MAX_JSON_DEPTH + 2 {
            nested = json!([nested]);
        }
        let value = json!({"label":"x","data":nested});
        assert!(
            parse_completion_result(&json!([value])).unwrap().candidates[0]
                .disabled_reason
                .as_ref()
                .unwrap()
                .contains("complex")
        );
    }

    #[test]
    fn range_conversion_handles_multiline_additional_edits() {
        let source = "aa\r\n中\r\nxx";
        assert_eq!(
            range_to_offsets(
                source,
                Range {
                    start: p(0, 2),
                    end: p(2, 0)
                }
            )
            .unwrap(),
            (2, 9)
        );
        assert!(range_to_offsets(
            source,
            Range {
                start: p(2, 0),
                end: p(0, 2)
            }
        )
        .is_err());
        let value = json!({"label":"X","textEdit":edit(2,0,2,"X"),"additionalTextEdits":[{"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":1}},"newText":"header"}]});
        assert_eq!(
            apply_completion(source, p(2, 2), &value).unwrap().text,
            "header\r\nX"
        );
    }
}
