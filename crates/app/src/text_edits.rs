//! A bounded, pure planner for plain LSP text edits against one captured source.
//!
//! This deliberately accepts neither workspace edits nor annotated edits,
//! commands, snippets, or resource operations. The caller owns snapshot checks,
//! preview, and committing the finished replacement as one editor transaction.

use crate::completion::{Position, Range};
use serde_json::{Map, Value};
use std::{iter::Peekable, str::CharIndices};

pub const MAX_TEXT_EDITS: usize = 1024;
pub const MAX_TEXT_EDIT_BYTES: usize = 1024 * 1024;
const MAX_POSITION: u32 = i32::MAX as u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedTextEdit {
    pub text: String,
    /// Unicode scalar offset, matching the editor's character cursor convention.
    pub cursor_chars: usize,
    /// Number of accepted proposals, including any individually unchanged edit.
    pub edit_count: usize,
}

fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
    name: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{name} must be an object"))?;
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(format!("{name} contains an unsupported field"));
    }
    if fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!("{name} is missing a required field"));
    }
    Ok(object)
}

fn parse_position(value: &Value) -> Result<Position, String> {
    let object = exact_object(value, &["line", "character"], "Text edit position")?;
    let integer = |field| {
        object[field]
            .as_u64()
            .filter(|value| *value <= u64::from(MAX_POSITION))
            .map(|value| value as u32)
            .ok_or_else(|| "Text edit positions must be unsigned 31-bit integers".to_owned())
    };
    Ok(Position {
        line: integer("line")?,
        character: integer("character")?,
    })
}

fn validate_range(range: Range) -> Result<(), String> {
    if [range.start, range.end]
        .iter()
        .any(|position| position.line > MAX_POSITION || position.character > MAX_POSITION)
    {
        return Err("Text edit positions must be unsigned 31-bit integers".into());
    }
    if range.start > range.end {
        return Err("Text edit range is reversed".into());
    }
    Ok(())
}

fn parse_edit(value: &Value) -> Result<(Range, &str), String> {
    let object = exact_object(value, &["range", "newText"], "Text edit")?;
    let range = exact_object(&object["range"], &["start", "end"], "Text edit range")?;
    let range = Range {
        start: parse_position(&range["start"])?,
        end: parse_position(&range["end"])?,
    };
    validate_range(range)?;
    let text = object["newText"]
        .as_str()
        .ok_or("Text edit newText must be a string")?;
    Ok((range, text))
}

fn add_inserted(total: usize, text: &str) -> Result<usize, String> {
    let total = total
        .checked_add(text.len())
        .filter(|total| *total <= MAX_TEXT_EDIT_BYTES)
        .ok_or("Aggregate text edit insertion exceeds the 1 MiB limit")?;
    if text.as_bytes().contains(&0) {
        return Err("Text edit insertion contains NUL".into());
    }
    Ok(total)
}

/// Parse an LSP `TextEdit[] | null`. Every nested object has an exact schema.
/// Validate the entire payload and aggregate size before cloning inserted text.
pub fn parse_text_edits(value: &Value) -> Result<Vec<TextEdit>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value
        .as_array()
        .ok_or("Text edits must be an array or null")?;
    if values.len() > MAX_TEXT_EDITS {
        return Err("Text edits exceed the 1024-edit limit".into());
    }
    let mut inserted = 0;
    for value in values {
        let (_, text) = parse_edit(value)?;
        inserted = add_inserted(inserted, text)?;
    }
    let mut edits = Vec::new();
    edits
        .try_reserve_exact(values.len())
        .map_err(|_| "Cannot allocate text edit proposals")?;
    for value in values {
        let (range, text) = parse_edit(value)?;
        let mut new_text = String::new();
        new_text
            .try_reserve_exact(text.len())
            .map_err(|_| "Cannot allocate text edit insertion")?;
        new_text.push_str(text);
        edits.push(TextEdit { range, new_text });
    }
    Ok(edits)
}

/// Resolves ordered UTF-16 positions in a single source scan. Unlike repeatedly
/// rebuilding a line index per edit, cost is O(source bytes + edit endpoints).
/// CRLF is consumed atomically; bare CR and LF each terminate one line.
struct SourceOffsets<'a> {
    characters: Peekable<CharIndices<'a>>,
    position: Position,
    byte: usize,
    chars: usize,
}

impl<'a> SourceOffsets<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            characters: source.char_indices().peekable(),
            position: Position::default(),
            byte: 0,
            chars: 0,
        }
    }

    fn advance_to(&mut self, target: Position) -> Result<(usize, usize), String> {
        while self.position < target {
            let (byte, character) = self
                .characters
                .next()
                .ok_or("Text edit position is outside the document")?;
            self.byte = byte + character.len_utf8();
            self.chars += 1;
            if character == '\r' || character == '\n' {
                if self.position.line == target.line {
                    return Err("Text edit character is past the end of its line".into());
                }
                if character == '\r' && self.characters.peek().is_some_and(|(_, c)| *c == '\n') {
                    self.characters.next();
                    self.byte += 1;
                    self.chars += 1;
                }
                self.position.line += 1;
                self.position.character = 0;
            } else {
                self.position.character += character.len_utf16() as u32;
            }
        }
        if self.position != target {
            return Err("Text edit position splits a UTF-16 surrogate pair".into());
        }
        Ok((self.byte, self.chars))
    }
}

fn cursor_byte(source: &str, chars: usize) -> Result<usize, String> {
    source
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(source.len()))
        .nth(chars)
        .ok_or_else(|| "Text edit cursor is outside the document".to_owned())
}

fn inside_crlf(source: &str, byte: usize) -> bool {
    byte > 0
        && source.as_bytes().get(byte - 1) == Some(&b'\r')
        && source.as_bytes().get(byte) == Some(&b'\n')
}

struct ResolvedEdit<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

/// Build a fully validated replacement without changing the source or editor.
///
/// All ranges refer to `source`; array order does not affect the result. Adjacent
/// nonempty ranges are valid, but insertions sharing any edit boundary are not.
/// Positions are strict UTF-16, while the captured cursor is a Unicode scalar
/// offset. Invalid cursors, including the interior of CRLF, are rejected.
///
/// Cursor policy: preserve offsets in unchanged text; place an insertion-time
/// cursor after its inserted text; preserve the relative scalar offset inside a
/// replacement, clamped to its length; map the replaced range's end to the new
/// end. Snap a cursor that lands inside a newly created CRLF to after its LF.
/// An unchanged final document always retains the captured cursor exactly.
///
/// Callers should compare `plan.text == source` to detect all no-op proposals,
/// and must recheck document/session identity and version before committing.
pub fn plan_text_edits(
    source: &str,
    edits: &[TextEdit],
    cursor_chars: usize,
) -> Result<PlannedTextEdit, String> {
    if source.len() > MAX_TEXT_EDIT_BYTES {
        return Err("Text edit source exceeds the 1 MiB limit".into());
    }
    if edits.len() > MAX_TEXT_EDITS {
        return Err("Text edits exceed the 1024-edit limit".into());
    }
    let original_cursor_byte = cursor_byte(source, cursor_chars)?;
    if inside_crlf(source, original_cursor_byte) {
        return Err("Text edit cursor is inside a CRLF terminator".into());
    }
    let mut inserted = 0;
    for edit in edits {
        validate_range(edit.range)?;
        inserted = add_inserted(inserted, &edit.new_text)?;
    }

    let mut ordered = Vec::new();
    ordered
        .try_reserve_exact(edits.len())
        .map_err(|_| "Cannot allocate ordered text edits")?;
    ordered.extend(edits.iter());
    ordered.sort_unstable_by_key(|edit| (edit.range.start, edit.range.end));
    for pair in ordered.windows(2) {
        let (left, right) = (pair[0].range, pair[1].range);
        if left.end > right.start
            || (left.end == right.start && (left.start == left.end || right.start == right.end))
        {
            return Err("Text edits overlap or share an ambiguous insertion boundary".into());
        }
    }

    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(edits.len())
        .map_err(|_| "Cannot allocate resolved text edits")?;
    let mut offsets = SourceOffsets::new(source);
    let (mut removed, mut output_chars, mut previous_end_chars) = (0_usize, 0_usize, 0_usize);
    let mut mapped_cursor = cursor_chars;
    for edit in ordered {
        let (start, start_chars) = offsets.advance_to(edit.range.start)?;
        let (end, end_chars) = offsets.advance_to(edit.range.end)?;
        removed = removed
            .checked_add(end - start)
            .ok_or("Text edit removal size overflow")?;
        let new_chars = edit.new_text.chars().count();
        let output_start = output_chars
            .checked_add(start_chars - previous_end_chars)
            .ok_or("Text edit cursor size overflow")?;
        output_chars = output_start
            .checked_add(new_chars)
            .ok_or("Text edit cursor size overflow")?;
        if cursor_chars >= start_chars {
            mapped_cursor = if cursor_chars >= end_chars {
                output_chars.checked_add(cursor_chars - end_chars)
            } else {
                output_start.checked_add((cursor_chars - start_chars).min(new_chars))
            }
            .ok_or("Text edit cursor size overflow")?;
        }
        previous_end_chars = end_chars;
        resolved.push(ResolvedEdit {
            start,
            end,
            text: &edit.new_text,
        });
    }
    let final_size = source
        .len()
        .checked_sub(removed)
        .and_then(|size| size.checked_add(inserted))
        .filter(|size| *size <= MAX_TEXT_EDIT_BYTES)
        .ok_or("Text edit result exceeds the 1 MiB limit")?;

    // No output buffer is constructed until every range and bound has passed.
    let mut text = String::new();
    text.try_reserve_exact(final_size)
        .map_err(|_| "Cannot allocate text edit result")?;
    let mut previous = 0;
    for edit in resolved {
        text.push_str(&source[previous..edit.start]);
        text.push_str(edit.text);
        previous = edit.end;
    }
    text.push_str(&source[previous..]);
    if text == source {
        mapped_cursor = cursor_chars;
    }
    let mapped_byte = cursor_byte(&text, mapped_cursor)?;
    if inside_crlf(&text, mapped_byte) {
        mapped_cursor = mapped_cursor
            .checked_add(1)
            .ok_or("Text edit cursor size overflow")?;
    }
    Ok(PlannedTextEdit {
        text,
        cursor_chars: mapped_cursor,
        edit_count: edits.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::{chars_to_position, position_to_offsets};
    use serde_json::json;

    fn p(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn edit(start: Position, end: Position, text: &str) -> TextEdit {
        TextEdit {
            range: Range { start, end },
            new_text: text.into(),
        }
    }

    fn line_edit(start: u32, end: u32, text: &str) -> TextEdit {
        edit(p(0, start), p(0, end), text)
    }

    fn value(start: u32, end: u32, text: &str) -> Value {
        json!({"range":{"start":{"line":0,"character":start},
            "end":{"line":0,"character":end}},"newText":text})
    }

    #[test]
    fn parses_null_empty_and_plain_edits() {
        assert!(parse_text_edits(&Value::Null).unwrap().is_empty());
        assert!(parse_text_edits(&json!([])).unwrap().is_empty());
        assert_eq!(
            parse_text_edits(&json!([value(0, 1, "中😀e\u{301}\r\n")])).unwrap(),
            vec![line_edit(0, 1, "中😀e\u{301}\r\n")]
        );
    }

    #[test]
    fn rejects_non_arrays_and_non_plain_edit_shapes() {
        for invalid in [
            json!(false),
            json!(1),
            json!("text"),
            json!({}),
            json!({"changes":{}}),
            json!({"documentChanges":[]}),
            json!([null]),
            json!([[]]),
            json!([{"kind":"rename","oldUri":"file:///a","newUri":"file:///b"}]),
            json!([{"insert":{},"replace":{},"newText":"x"}]),
        ] {
            assert!(parse_text_edits(&invalid).is_err(), "{invalid}");
        }
        for key in [
            "annotationId",
            "command",
            "data",
            "kind",
            "snippet",
            "label",
        ] {
            let mut proposal = value(0, 0, "x");
            proposal[key] = Value::Null;
            assert!(parse_text_edits(&json!([proposal])).is_err(), "{key}");
        }
    }

    #[test]
    fn rejects_missing_malformed_and_unknown_nested_fields() {
        for invalid in [
            json!({"newText":"x"}),
            json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}),
            json!({"range":null,"newText":"x"}),
            json!({"range":{},"newText":"x"}),
            json!({"range":{"start":null,"end":{}},"newText":"x"}),
        ] {
            assert!(parse_text_edits(&json!([invalid])).is_err());
        }
        for path in [vec!["range"], vec!["range", "start"], vec!["range", "end"]] {
            let mut proposal = value(0, 0, "x");
            let mut node = &mut proposal;
            for part in path {
                node = &mut node[part];
            }
            node["unexpected"] = json!(true);
            assert!(parse_text_edits(&json!([proposal])).is_err());
        }
        for invalid in [Value::Null, json!(0), json!([]), json!({}), json!(true)] {
            let mut proposal = value(0, 0, "x");
            proposal["newText"] = invalid;
            assert!(parse_text_edits(&json!([proposal])).is_err());
        }
        for field in ["line", "character"] {
            let mut proposal = value(0, 0, "x");
            proposal["range"]["start"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(parse_text_edits(&json!([proposal])).is_err());
        }
    }

    #[test]
    fn unsigned_31_bit_position_bounds_are_exact() {
        for endpoint in ["start", "end"] {
            for field in ["line", "character"] {
                for invalid in [
                    json!(-1),
                    json!(1.0),
                    json!(i64::MIN),
                    json!(u64::from(MAX_POSITION) + 1),
                    json!(u64::MAX),
                    json!("0"),
                    Value::Null,
                    json!(true),
                ] {
                    let mut proposal = value(0, 0, "x");
                    proposal["range"][endpoint][field] = invalid;
                    assert!(parse_text_edits(&json!([proposal])).is_err());
                }
            }
        }
        let mut proposal = value(0, MAX_POSITION, "");
        proposal["range"]["end"]["line"] = json!(MAX_POSITION);
        assert!(parse_text_edits(&json!([proposal])).is_ok());
        assert!(plan_text_edits("", &[line_edit(0, MAX_POSITION + 1, "")], 0).is_err());
        assert!(plan_text_edits("", &[edit(p(0, 0), p(MAX_POSITION + 1, 0), "")], 0).is_err());
    }

    #[test]
    fn rejects_reversed_ranges_at_both_entry_points() {
        assert!(parse_text_edits(&json!([value(2, 1, "")])).is_err());
        assert!(plan_text_edits("abc", &[line_edit(2, 1, "")], 0).is_err());
        assert!(plan_text_edits("a\nb", &[edit(p(1, 0), p(0, 1), "")], 0).is_err());
    }

    #[test]
    fn rejects_nul_before_owning_or_applying_text() {
        assert!(parse_text_edits(&json!([value(0, 0, "a\0b")])).is_err());
        assert!(plan_text_edits("abc", &[line_edit(0, 0, "a\0b")], 0).is_err());
        assert_eq!(plan_text_edits("a\0b", &[], 1).unwrap().text, "a\0b");
    }

    #[test]
    fn preserves_unicode_and_mixed_line_endings_exactly() {
        let source = "😀中e\u{301}\r\n甲\n乙\r末";
        let edits = [
            edit(p(3, 1), p(3, 1), "!"),
            edit(p(1, 0), p(2, 1), "新\r\n字"),
            edit(p(0, 2), p(0, 5), "文e\u{301}"),
        ];
        let plan = plan_text_edits(source, &edits, source.chars().count()).unwrap();
        assert_eq!(plan.text, "😀文e\u{301}\r\n新\r\n字\r末!");
        assert_eq!(plan.cursor_chars, plan.text.chars().count());
        assert_eq!(plan.edit_count, 3);
    }

    #[test]
    fn utf16_surrogate_splits_and_past_line_positions_are_rejected() {
        for source in ["a😀b", "a😀b\n", "a😀b\r\n", "a😀b\r"] {
            for range in [
                (p(0, 2), p(0, 2)),
                (p(0, 0), p(0, 2)),
                (p(0, 2), p(0, 3)),
                (p(0, 5), p(0, 5)),
                (p(5, 0), p(5, 0)),
            ] {
                assert!(plan_text_edits(source, &[edit(range.0, range.1, "")], 0).is_err());
            }
        }
        assert_eq!(
            plan_text_edits("a😀b", &[line_edit(1, 3, "中")], 2)
                .unwrap()
                .text,
            "a中b"
        );
    }

    #[test]
    fn crlf_interiors_are_never_positions_or_valid_cursors() {
        for position in [p(0, 2), p(0, 3), p(1, 2)] {
            assert!(plan_text_edits("a\r\nb", &[edit(position, position, "")], 0).is_err());
        }
        assert!(plan_text_edits("a\r\nb", &[], 2).is_err());
        let plan = plan_text_edits("a\r\nb", &[edit(p(0, 1), p(1, 0), "\n")], 3).unwrap();
        assert_eq!((plan.text.as_str(), plan.cursor_chars), ("a\nb", 2));
    }

    #[test]
    fn eof_insertions_and_empty_last_lines_are_valid() {
        for (source, end) in [
            ("", p(0, 0)),
            ("中😀", p(0, 3)),
            ("a\n", p(1, 0)),
            ("a\r", p(1, 0)),
            ("a\r\n", p(1, 0)),
        ] {
            let plan = plan_text_edits(source, &[edit(end, end, "\r\n中")], source.chars().count())
                .unwrap();
            assert_eq!(plan.text, format!("{source}\r\n中"));
            assert_eq!(plan.cursor_chars, plan.text.chars().count());
        }
    }

    #[test]
    fn rejects_overlap_nested_ranges_and_every_shared_insertion_boundary() {
        for edits in [
            vec![line_edit(1, 4, "a"), line_edit(2, 3, "b")],
            vec![line_edit(1, 3, "a"), line_edit(2, 4, "b")],
            vec![line_edit(1, 3, "a"), line_edit(1, 3, "b")],
            vec![line_edit(2, 2, "a"), line_edit(2, 2, "b")],
            vec![line_edit(1, 3, "a"), line_edit(1, 1, "b")],
            vec![line_edit(1, 3, "a"), line_edit(3, 3, "b")],
            vec![line_edit(1, 3, "a"), line_edit(2, 2, "b")],
            vec![line_edit(1, 1, ""), line_edit(1, 1, "")],
        ] {
            assert!(plan_text_edits("abcde", &edits, 0)
                .unwrap_err()
                .contains("overlap"));
            let reversed = edits.into_iter().rev().collect::<Vec<_>>();
            assert!(plan_text_edits("abcde", &reversed, 0).is_err());
        }
    }

    #[test]
    fn adjacent_nonempty_edits_are_valid_and_order_independent() {
        let edits = [line_edit(2, 4, "😀"), line_edit(0, 2, "中")];
        let plan = plan_text_edits("abcd!", &edits, 2).unwrap();
        assert_eq!((plan.text.as_str(), plan.cursor_chars), ("中😀!", 1));
        let ordered = [edits[1].clone(), edits[0].clone()];
        assert_eq!(plan_text_edits("abcd!", &ordered, 2).unwrap(), plan);
    }

    #[test]
    fn ranges_always_refer_to_the_original_source() {
        let plan = plan_text_edits(
            "abcdef",
            &[
                line_edit(4, 6, "末"),
                line_edit(0, 1, "long"),
                line_edit(2, 3, ""),
            ],
            6,
        )
        .unwrap();
        assert_eq!((plan.text.as_str(), plan.cursor_chars), ("longbd末", 7));
    }

    #[test]
    fn cursor_mapping_has_right_insertion_affinity() {
        for (cursor, expected) in [(0, 0), (1, 3), (2, 4), (3, 5)] {
            let plan = plan_text_edits("abc", &[line_edit(1, 1, "😀中")], cursor).unwrap();
            assert_eq!(
                (plan.text.as_str(), plan.cursor_chars),
                ("a😀中bc", expected)
            );
        }
    }

    #[test]
    fn replacement_cursor_preserves_relative_scalars_and_clamps() {
        for (replacement, mappings) in [
            ("😀中", [0, 1, 2, 3, 3, 4]),
            ("", [0, 1, 1, 1, 1, 2]),
            ("vwxyz", [0, 1, 2, 3, 6, 7]),
        ] {
            for (cursor, expected) in mappings.into_iter().enumerate() {
                assert_eq!(
                    plan_text_edits("abcde", &[line_edit(1, 4, replacement)], cursor)
                        .unwrap()
                        .cursor_chars,
                    expected,
                    "replacement={replacement:?}, cursor={cursor}"
                );
            }
        }
    }

    #[test]
    fn cursor_deltas_account_for_edits_before_but_not_after() {
        let edits = [
            line_edit(0, 1, "中😀"),
            line_edit(3, 4, ""),
            line_edit(6, 6, "!"),
        ];
        for (cursor, expected) in [(0, 0), (1, 2), (2, 3), (3, 4), (4, 4), (5, 5), (6, 7)] {
            let plan = plan_text_edits("abcdef", &edits, cursor).unwrap();
            assert_eq!(
                (plan.text.as_str(), plan.cursor_chars),
                ("中😀bcef!", expected)
            );
        }
    }

    #[test]
    fn replacement_cursor_counts_combining_marks_and_astral_scalars() {
        let source = "😀中e\u{301}末";
        let edits = [line_edit(2, 5, "文😀字")];
        for cursor in 0..=5 {
            let plan = plan_text_edits(source, &edits, cursor).unwrap();
            assert_eq!(plan.text, "😀文😀字末");
            assert_eq!(plan.cursor_chars, cursor);
        }
    }

    #[test]
    fn snaps_new_crlf_interiors_in_replacements_and_at_boundaries() {
        let plan = plan_text_edits("abcd", &[line_edit(0, 4, "a\r\nb")], 2).unwrap();
        assert_eq!((plan.text.as_str(), plan.cursor_chars), ("a\r\nb", 3));
        let plan = plan_text_edits("a\rb", &[edit(p(1, 0), p(1, 1), "\nx")], 2).unwrap();
        assert_eq!((plan.text.as_str(), plan.cursor_chars), ("a\r\nx", 3));
        assert!(chars_to_position(&plan.text, plan.cursor_chars).is_ok());
    }

    #[test]
    fn identical_results_and_empty_edits_preserve_the_cursor() {
        let source = "😀中\r\ne\u{301}\r末\n";
        let whole = edit(p(0, 0), p(3, 0), source);
        for cursor in 0..=source.chars().count() {
            if chars_to_position(source, cursor).is_err() {
                continue;
            }
            for edits in [Vec::new(), vec![whole.clone()]] {
                let plan = plan_text_edits(source, &edits, cursor).unwrap();
                assert_eq!(plan.text, source);
                assert_eq!(plan.cursor_chars, cursor);
                assert_eq!(plan.edit_count, edits.len());
            }
        }
        let plan = plan_text_edits("abc", &[line_edit(1, 1, "")], 1).unwrap();
        assert_eq!(
            (plan.text.as_str(), plan.cursor_chars, plan.edit_count),
            ("abc", 1, 1)
        );
        // Individually changed adjacent ranges can still cancel out globally.
        for cursor in 0..=4 {
            let plan = plan_text_edits(
                "aabb",
                &[line_edit(0, 2, "a"), line_edit(2, 4, "abb")],
                cursor,
            )
            .unwrap();
            assert_eq!((plan.text.as_str(), plan.cursor_chars), ("aabb", cursor));
        }
    }

    #[test]
    fn invalid_cursor_is_not_silently_clamped() {
        for cursor in [1, usize::MAX] {
            assert!(plan_text_edits("", &[], cursor).is_err());
        }
        assert!(plan_text_edits("😀", &[], 2).is_err());
        assert!(plan_text_edits("a\r\nb", &[line_edit(0, 1, "x")], 2).is_err());
    }

    #[test]
    fn source_and_result_byte_limits_are_inclusive() {
        let limit = "a".repeat(MAX_TEXT_EDIT_BYTES);
        assert_eq!(
            plan_text_edits(&limit, &[], 0).unwrap().text.len(),
            MAX_TEXT_EDIT_BYTES
        );
        assert!(plan_text_edits(&(limit.clone() + "a"), &[], 0).is_err());
        assert!(plan_text_edits(&limit, &[line_edit(0, 0, "a")], 0).is_err());
        let plan = plan_text_edits(&limit, &[line_edit(0, 1, "b")], 0).unwrap();
        assert_eq!(plan.text.len(), MAX_TEXT_EDIT_BYTES);
        let plan = plan_text_edits("", &[line_edit(0, 0, &limit)], 0).unwrap();
        assert_eq!(plan.cursor_chars, MAX_TEXT_EDIT_BYTES);
    }

    #[test]
    fn insertion_limit_is_aggregate_even_when_result_would_fit() {
        let half = "a".repeat(MAX_TEXT_EDIT_BYTES / 2);
        let source = "x".repeat(MAX_TEXT_EDIT_BYTES);
        let edits = [
            line_edit(0, 1, &half),
            line_edit(1, MAX_TEXT_EDIT_BYTES as u32, &(half.clone() + "a")),
        ];
        assert!(plan_text_edits(&source, &edits, 0)
            .unwrap_err()
            .contains("Aggregate"));
        assert!(parse_text_edits(&json!([
            value(0, 1, &half),
            value(1, 2, &(half.clone() + "a"))
        ]))
        .is_err());
        assert!(parse_text_edits(&json!([value(0, 1, &half), value(1, 2, &half)])).is_ok());
        let oversized = "😀".repeat(MAX_TEXT_EDIT_BYTES / 4 + 1);
        assert!(parse_text_edits(&json!([value(0, 0, &oversized)])).is_err());
        assert!(plan_text_edits("", &[line_edit(0, 0, &oversized)], 0).is_err());
    }

    #[test]
    fn edit_count_limit_is_inclusive_at_both_entry_points() {
        let edits = (0..MAX_TEXT_EDITS)
            .map(|i| line_edit(i as u32, i as u32 + 1, "b"))
            .collect::<Vec<_>>();
        let source = "a".repeat(MAX_TEXT_EDITS);
        assert_eq!(
            plan_text_edits(&source, &edits, 0).unwrap().text,
            "b".repeat(MAX_TEXT_EDITS)
        );
        let mut too_many = edits;
        too_many.push(line_edit(MAX_TEXT_EDITS as u32, MAX_TEXT_EDITS as u32, ""));
        assert!(plan_text_edits(&source, &too_many, 0).is_err());
        let mut values = vec![value(0, 0, ""); MAX_TEXT_EDITS];
        assert_eq!(
            parse_text_edits(&json!(values)).unwrap().len(),
            MAX_TEXT_EDITS
        );
        values.push(value(0, 0, ""));
        assert!(parse_text_edits(&json!(values)).is_err());
    }

    #[test]
    fn invalid_late_edit_never_changes_the_captured_source() {
        let source = "unchanged".to_owned();
        assert!(plan_text_edits(
            &source,
            &[line_edit(0, 1, "changed"), line_edit(20, 20, "x")],
            3
        )
        .is_err());
        assert_eq!(source, "unchanged");
        assert!(parse_text_edits(&json!([value(0, 1, "changed"), {"command":"bad"}])).is_err());
    }

    #[test]
    fn ordered_resolver_matches_shared_strict_utf16_conversions() {
        for source in [
            "",
            "a",
            "😀中e\u{301}",
            "a\r\nb\rc\n",
            "\r\n\r\n",
            "😀\n中\r末",
        ] {
            for line in 0..6 {
                for character in 0..9 {
                    let position = p(line, character);
                    let expected = position_to_offsets(source, position);
                    let actual = SourceOffsets::new(source).advance_to(position);
                    assert_eq!(
                        actual.is_ok(),
                        expected.is_ok(),
                        "source={source:?}, position={position:?}"
                    );
                    if let (Ok(actual), Ok(expected)) = (actual, expected) {
                        assert_eq!(actual, expected);
                    }
                }
            }
        }
    }

    #[test]
    fn small_unicode_edits_match_reference_slices_and_keep_valid_cursors() {
        let alphabet = ['a', '😀', '中', '\u{301}', '\r', '\n'];
        for seed in 0_usize..64 {
            let mut number = seed * 7919;
            let source = (0..seed % 7)
                .map(|_| {
                    let character = alphabet[number % alphabet.len()];
                    number /= alphabet.len();
                    character
                })
                .collect::<String>();
            let boundaries = (0..=source.chars().count())
                .filter_map(|chars| {
                    chars_to_position(&source, chars)
                        .ok()
                        .map(|position| (chars, position))
                })
                .collect::<Vec<_>>();
            for (start_index, (_, start)) in boundaries.iter().enumerate() {
                for (_, end) in &boundaries[start_index..] {
                    let start_byte = position_to_offsets(&source, *start).unwrap().0;
                    let end_byte = position_to_offsets(&source, *end).unwrap().0;
                    for replacement in ["", "x", "😀", "中e\u{301}", "\r\n", "\n", "\r"] {
                        let expected = format!(
                            "{}{replacement}{}",
                            &source[..start_byte],
                            &source[end_byte..]
                        );
                        for (cursor, _) in &boundaries {
                            let plan = plan_text_edits(
                                &source,
                                &[edit(*start, *end, replacement)],
                                *cursor,
                            )
                            .unwrap();
                            assert_eq!(plan.text, expected);
                            assert!(chars_to_position(&plan.text, plan.cursor_chars).is_ok());
                            if plan.text == source {
                                assert_eq!(plan.cursor_chars, *cursor);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn maximum_source_and_edit_count_use_one_ordered_scan() {
        let source = "a\r\n".repeat(MAX_TEXT_EDIT_BYTES / 3);
        let source_lines = source.len() / 3;
        let edits = (0..MAX_TEXT_EDITS)
            .rev()
            .map(|index| {
                let line = (index * source_lines / MAX_TEXT_EDITS) as u32;
                edit(p(line, 0), p(line, 1), "b")
            })
            .collect::<Vec<_>>();
        let plan = plan_text_edits(&source, &edits, source.chars().count()).unwrap();
        assert_eq!(plan.text.len(), source.len());
        assert_eq!(
            plan.text.bytes().filter(|byte| *byte == b'b').count(),
            MAX_TEXT_EDITS
        );
        assert_eq!(plan.cursor_chars, source.chars().count());
    }
}
