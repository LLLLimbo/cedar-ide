//! Strict, bounded, inert navigation results. URIs are never opened by this module.
use crate::completion::{position_to_offsets, Position, Range};
use crate::language_results::{parse_range, Location};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub const MAX_REFERENCES: usize = 1_024;
pub const MAX_OUTLINE_ITEMS: usize = 2_000;
/// Roots have depth zero.
pub const MAX_OUTLINE_DEPTH: usize = 32;
const MAX_URI_BYTES: usize = 16 * 1024;
const MAX_RETAINED_BYTES: usize = 512 * 1024;
pub const MAX_WORKSPACE_SYMBOLS: usize = 256;

#[derive(Clone, Debug)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub container: String,
    pub kind: u32,
    pub deprecated: bool,
    pub location: Location,
}

pub fn validate_workspace_query(query: &str) -> Result<(), String> {
    if query.trim().is_empty() || query.len() > 256 || query.chars().any(char::is_control) {
        return Err("Enter 1–256 UTF-8 bytes without control characters".into());
    }
    Ok(())
}

/// Accept only complete, bounded SymbolInformation rows. No URI or extension
/// is executed, resolved or opened while parsing this unversioned index snapshot.
pub fn parse_workspace_symbols(value: &Value) -> Result<Vec<WorkspaceSymbol>, String> {
    use std::io::{self, Write};
    struct Budget(usize);
    impl Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.checked_sub(bytes.len()).ok_or_else(|| {
                io::Error::other("Workspace symbols exceed the 1 MiB response limit")
            })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut stack = vec![(value, 0)];
    let mut nodes = 0;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        let children = match value {
            Value::Array(values) => values.len(),
            Value::Object(values) => values.len(),
            _ => 0,
        };
        if depth > 8 || nodes + children + stack.len() > 16 * 1024 {
            return Err("Workspace symbols exceed the response shape limit".into());
        }
        match value {
            Value::Array(values) => stack.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => stack.extend(values.values().map(|value| (value, depth + 1))),
            _ => {}
        }
    }
    serde_json::to_writer(Budget(1024 * 1024), value)
        .map_err(|_| "Workspace symbols exceed the 1 MiB response limit")?;
    if value.is_null() {
        return Ok(vec![]);
    }
    let values = value
        .as_array()
        .ok_or("Workspace symbols must be a flat array or null")?;
    if values.len() > MAX_WORKSPACE_SYMBOLS {
        return Err("More than 256 workspace symbols; narrow the query".into());
    }
    let mut budget = TextBudget::default();
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        let map = object(value, "Workspace symbol")?;
        if ["children", "range", "selectionRange", "detail", "data"]
            .iter()
            .any(|key| map.contains_key(*key))
        {
            return Err(
                "Expected complete flat SymbolInformation; lazy symbol resolution is unsupported"
                    .into(),
            );
        }
        let name = budget.retain(required(map, "name")?, "Symbol name", 4096)?;
        let container = match map.get("containerName") {
            Some(value) => budget.retain(value, "Symbol container", 4096)?,
            None => String::new(),
        };
        if name.is_empty() || name.chars().chain(container.chars()).any(char::is_control) {
            return Err("Symbol names and containers must be plain text without controls".into());
        }
        let kind = required(map, "kind")?
            .as_u64()
            .filter(|kind| (1..=26).contains(kind))
            .ok_or("Symbol kind must be an integer in 1..=26")? as u32;
        let location = location(required(map, "location")?, &mut budget)?;
        let scheme = location.uri.split_once(':').map(|(scheme, _)| scheme);
        if location
            .uri
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
            || !scheme.is_some_and(|scheme| {
                scheme
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                    && scheme
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
            })
        {
            return Err(
                "Symbol location must be an absolute URI without whitespace or controls".into(),
            );
        }
        let tagged = match map.get("tags") {
            None => false,
            Some(value) => {
                let tags = value
                    .as_array()
                    .filter(|tags| {
                        tags.len() <= 1 && tags.iter().all(|tag| tag.as_u64() == Some(1))
                    })
                    .ok_or("Invalid workspace symbol tags")?;
                !tags.is_empty()
            }
        };
        let deprecated = match map.get("deprecated") {
            None => tagged,
            Some(value) => {
                value
                    .as_bool()
                    .ok_or("Invalid workspace symbol deprecated flag")?
                    || tagged
            }
        };
        rows.push(WorkspaceSymbol {
            name,
            container,
            kind,
            deprecated,
            location,
        });
    }
    rows.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.container.cmp(&b.container))
            .then_with(|| a.location.uri.cmp(&b.location.uri))
            .then_with(|| a.location.range.start.cmp(&b.location.range.start))
            .then_with(|| a.location.range.end.cmp(&b.location.range.end))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.deprecated.cmp(&b.deprecated))
    });
    Ok(rows)
}

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub name: String,
    /// A document symbol's detail or a flat symbol's container name, for display only.
    pub detail: String,
    pub kind: u32,
    pub depth: usize,
    pub location: OutlineLocation,
}

#[derive(Clone, Debug)]
pub enum OutlineLocation {
    Local { range: Range, selection: Range },
    Remote(Location),
}

#[derive(Default)]
struct TextBudget(usize);

impl TextBudget {
    fn retain(&mut self, value: &Value, label: &str, limit: usize) -> Result<String, String> {
        let text = value
            .as_str()
            .ok_or_else(|| format!("{label} must be a string"))?;
        if text.len() > limit {
            return Err(format!("{label} exceeds its text limit"));
        }
        let total = self
            .0
            .checked_add(text.len())
            .filter(|total| *total <= MAX_RETAINED_BYTES)
            .ok_or("Navigation text exceeds the 512 KiB display limit")?;
        self.0 = total;
        Ok(text.to_owned())
    }
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    map.get(key)
        .ok_or_else(|| format!("Navigation result is missing {key}"))
}

fn location(value: &Value, budget: &mut TextBudget) -> Result<Location, String> {
    let map = object(value, "Location")?;
    if [
        "targetUri",
        "targetRange",
        "targetSelectionRange",
        "originSelectionRange",
    ]
    .iter()
    .any(|key| map.contains_key(*key))
    {
        return Err("Expected a Location, not a LocationLink".into());
    }
    let uri = budget.retain(required(map, "uri")?, "Location URI", MAX_URI_BYTES)?;
    if uri.is_empty() {
        return Err("Location URI must not be empty".into());
    }
    Ok(Location {
        uri,
        range: parse_range(required(map, "range")?)?,
    })
}

/// References are exactly Location[] | null; a single Location or LocationLink
/// is not a references response. No malformed or oversized response is truncated.
pub fn parse_references(value: &Value) -> Result<Vec<Location>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value
        .as_array()
        .ok_or("References result must be an array or null")?;
    if values.len() > MAX_REFERENCES {
        return Err("References result exceeds the 1024-location limit".into());
    }
    let mut budget = TextBudget::default();
    values
        .iter()
        .map(|value| location(value, &mut budget))
        .collect()
}

fn contains(outer: Range, inner: Range) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Validate sorted unique endpoints without rebuilding a whole-document index
/// for every symbol. The shared converter sees only a bounded prefix between
/// consecutive positions, so total scanning stays proportional to source size.
fn validate_positions(source: &str, positions: &BTreeSet<Position>) -> Result<(), String> {
    let mut remaining = source;
    let mut finished = false;
    let mut lines = std::iter::from_fn(move || {
        if finished {
            return None;
        }
        if let Some(end) = remaining.find(['\r', '\n']) {
            let line = &remaining[..end];
            let width = if remaining.as_bytes()[end] == b'\r'
                && remaining.as_bytes().get(end + 1) == Some(&b'\n')
            {
                2
            } else {
                1
            };
            remaining = &remaining[end + width..];
            Some(line)
        } else {
            finished = true;
            Some(remaining)
        }
    });
    let mut current: Option<&str> = None;
    let mut line_number = 0;
    let mut character = 0;
    for position in positions {
        if current.is_none() || position.line != line_number {
            let skip = if current.is_none() {
                position.line
            } else {
                position.line - line_number - 1
            };
            current = Some(
                lines
                    .nth(skip as usize)
                    .ok_or("Outline position has an out-of-range line")?,
            );
            line_number = position.line;
            character = 0;
        }
        let line = current.expect("current line was established above");
        let delta = position.character - character;
        // One UTF-16 unit requires at most three UTF-8 bytes; four also covers
        // an entire surrogate-pair scalar so the converter can reject its middle.
        let mut prefix = line.len().min((delta as usize).saturating_mul(4));
        while !line.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let (bytes, _) = position_to_offsets(
            &line[..prefix],
            Position {
                line: 0,
                character: delta,
            },
        )
        .map_err(|error| format!("Invalid outline source position: {error}"))?;
        current = Some(&line[bytes..]);
        character = position.character;
    }
    Ok(())
}

struct OutlineParser {
    budget: TextBudget,
    items: Vec<OutlineItem>,
    /// Shared boundaries are common in hierarchical outlines. Validate each once.
    positions: BTreeSet<Position>,
}

impl OutlineParser {
    fn remember_range(&mut self, range: Range) {
        self.positions.extend([range.start, range.end]);
    }

    fn parse_items(
        &mut self,
        values: &[Value],
        hierarchical: bool,
        depth: usize,
        parent: Option<Range>,
    ) -> Result<(), String> {
        if depth > MAX_OUTLINE_DEPTH && !values.is_empty() {
            return Err("Outline exceeds the depth-32 limit".into());
        }
        if values.len() > MAX_OUTLINE_ITEMS.saturating_sub(self.items.len()) {
            return Err("Outline exceeds the 2000-symbol limit".into());
        }
        for value in values {
            // Descendants of a previous sibling may have exhausted the budget.
            if self.items.len() >= MAX_OUTLINE_ITEMS {
                return Err("Outline exceeds the 2000-symbol limit".into());
            }
            let map = object(value, "Outline symbol")?;
            if hierarchical {
                if map.contains_key("location") || map.contains_key("containerName") {
                    return Err("Outline mixes DocumentSymbol and SymbolInformation shapes".into());
                }
            } else if ["range", "selectionRange", "children", "detail"]
                .iter()
                .any(|key| map.contains_key(*key))
            {
                return Err("Outline mixes DocumentSymbol and SymbolInformation shapes".into());
            }
            let name =
                self.budget
                    .retain(required(map, "name")?, "Symbol name", MAX_RETAINED_BYTES)?;
            if name.trim().is_empty() {
                return Err("Symbol name must not be empty".into());
            }
            let kind = required(map, "kind")?
                .as_u64()
                .and_then(|kind| u32::try_from(kind).ok())
                .ok_or("Symbol kind must be an unsigned 32-bit integer")?;
            let detail_key = if hierarchical {
                "detail"
            } else {
                "containerName"
            };
            let detail = match map.get(detail_key) {
                Some(value) => self.budget.retain(value, detail_key, MAX_RETAINED_BYTES)?,
                None => String::new(),
            };
            if hierarchical {
                let range = parse_range(required(map, "range")?)?;
                let selection = parse_range(required(map, "selectionRange")?)?;
                if !contains(range, selection) {
                    return Err("Outline selection is outside its symbol range".into());
                }
                if parent.is_some_and(|parent| !contains(parent, range)) {
                    return Err("Outline child is outside its parent range".into());
                }
                self.remember_range(range);
                self.remember_range(selection);
                self.items.push(OutlineItem {
                    name,
                    detail,
                    kind,
                    depth,
                    location: OutlineLocation::Local { range, selection },
                });
                if let Some(children) = map.get("children") {
                    let children = children
                        .as_array()
                        .ok_or("Outline children must be an array")?;
                    self.parse_items(children, true, depth + 1, Some(range))?;
                }
            } else {
                let location = location(required(map, "location")?, &mut self.budget)?;
                self.items.push(OutlineItem {
                    name,
                    detail,
                    kind,
                    depth: 0,
                    location: OutlineLocation::Remote(location),
                });
            }
        }
        Ok(())
    }
}

/// Parse one homogeneous LSP outline into bounded preorder display rows.
/// Hierarchical ranges refer to the captured source; flat locations are inert
/// remote targets and must be validated again by the navigation owner on open.
pub fn parse_outline(value: &Value, source: &str) -> Result<Vec<OutlineItem>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value
        .as_array()
        .ok_or("Outline result must be an array or null")?;
    let Some(first) = values.first() else {
        return Ok(Vec::new());
    };
    let hierarchical = !object(first, "Outline symbol")?.contains_key("location");
    let mut parser = OutlineParser {
        budget: TextBudget::default(),
        items: Vec::with_capacity(values.len().min(MAX_OUTLINE_ITEMS)),
        positions: BTreeSet::new(),
    };
    parser.parse_items(values, hierarchical, 0, None)?;
    validate_positions(source, &parser.positions)?;
    Ok(parser.items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn range(start: u32, end: u32) -> Value {
        json!({"start":{"line":0,"character":start},"end":{"line":0,"character":end}})
    }

    fn reference() -> Value {
        json!({"uri":"file:///workspace/main.rs","range":range(0, 1)})
    }

    fn symbol(name: &str) -> Value {
        json!({"name":name,"kind":12,"range":range(0, 3),"selectionRange":range(1, 2)})
    }

    fn flat_symbol() -> Value {
        json!({"name":"call","kind":12,"location":reference(),"containerName":"Module"})
    }

    #[test]
    fn null_and_empty_results_are_empty() {
        for value in [Value::Null, json!([])] {
            assert!(parse_references(&value).unwrap().is_empty());
            assert!(parse_outline(&value, "").unwrap().is_empty());
        }
    }

    #[test]
    fn references_accept_locations_without_source_validation() {
        let mut remote = reference();
        remote["range"]["start"]["line"] = json!(123);
        remote["range"]["end"]["line"] = json!(124);
        let result = parse_references(&json!([remote])).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uri, "file:///workspace/main.rs");
        assert_eq!(result[0].range.start.line, 123);
    }

    #[test]
    fn references_reject_single_locations_links_and_mixed_links() {
        assert!(parse_references(&reference()).is_err());
        let link = json!({"targetUri":"file:///x","targetRange":range(0, 1),"targetSelectionRange":range(0, 1)});
        assert!(parse_references(&json!([link.clone()])).is_err());
        assert!(parse_references(&json!([reference(), link])).is_err());
        let mut hybrid = reference();
        hybrid["targetUri"] = json!("file:///other");
        assert!(parse_references(&json!([hybrid])).is_err());
    }

    #[test]
    fn references_reject_malformed_shapes_and_uris() {
        for value in [
            json!(true),
            json!(1),
            json!("x"),
            json!([null]),
            json!([[]]),
        ] {
            assert!(parse_references(&value).is_err());
        }
        for uri in [Value::Null, json!(8), json!(""), json!(["file:///x"])] {
            let mut item = reference();
            item["uri"] = uri;
            assert!(parse_references(&json!([item])).is_err());
        }
        for key in ["uri", "range"] {
            let mut item = reference();
            item.as_object_mut().unwrap().remove(key);
            assert!(parse_references(&json!([item])).is_err());
        }
    }

    #[test]
    fn references_reject_invalid_integer_positions_and_reversed_ranges() {
        for invalid in [
            json!(-1),
            json!(0.5),
            json!(1.0),
            json!("1"),
            json!(true),
            Value::Null,
            json!(u32::MAX),
            json!(u64::MAX),
        ] {
            for side in ["start", "end"] {
                for key in ["line", "character"] {
                    let mut item = reference();
                    item["range"][side][key] = invalid.clone();
                    assert!(parse_references(&json!([item])).is_err());
                }
            }
        }
        let mut item = reference();
        item["range"] = range(2, 1);
        assert!(parse_references(&json!([item])).is_err());
        item["range"] = json!({"start":{"line":0,"character":0}});
        assert!(parse_references(&json!([item])).is_err());
    }

    #[test]
    fn references_enforce_count_uri_and_aggregate_limits_inclusively() {
        assert_eq!(
            parse_references(&json!(vec![reference(); MAX_REFERENCES]))
                .unwrap()
                .len(),
            MAX_REFERENCES
        );
        assert!(parse_references(&json!(vec![reference(); MAX_REFERENCES + 1])).is_err());
        let mut item = reference();
        item["uri"] = json!("u".repeat(MAX_URI_BYTES));
        assert!(parse_references(&json!([item.clone()])).is_ok());
        let full = vec![item.clone(); MAX_RETAINED_BYTES / MAX_URI_BYTES];
        assert!(parse_references(&json!(full.clone())).is_ok());
        let mut too_much = full;
        too_much.push(reference());
        assert!(parse_references(&json!(too_much)).is_err());
        item["uri"] = json!("u".repeat(MAX_URI_BYTES + 1));
        assert!(parse_references(&json!([item])).is_err());
    }

    #[test]
    fn hierarchical_outline_preserves_preorder_and_details() {
        let mut parent = symbol("parent");
        parent["detail"] = json!("fn parent()");
        parent["children"] = json!([symbol("first"), symbol("second")]);
        let rows = parse_outline(&json!([parent, symbol("sibling")]), "abc").unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.name.as_str(), row.depth))
                .collect::<Vec<_>>(),
            vec![("parent", 0), ("first", 1), ("second", 1), ("sibling", 0)]
        );
        assert_eq!(rows[0].detail, "fn parent()");
        assert_eq!(rows[0].kind, 12);
        assert!(matches!(rows[0].location, OutlineLocation::Local { .. }));
    }

    #[test]
    fn flat_outline_preserves_remote_locations_and_container_context() {
        // Remote positions must not be tested against this unrelated source.
        let rows = parse_outline(&json!([flat_symbol()]), "").unwrap();
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[0].detail, "Module");
        match &rows[0].location {
            OutlineLocation::Remote(location) => {
                assert_eq!(location.uri, "file:///workspace/main.rs")
            }
            OutlineLocation::Local { .. } => panic!("flat symbol became local"),
        }
        let mut flat = flat_symbol();
        flat.as_object_mut().unwrap().remove("containerName");
        assert!(parse_outline(&json!([flat]), "").unwrap()[0]
            .detail
            .is_empty());
    }

    #[test]
    fn outline_rejects_mixed_top_level_nested_and_hybrid_shapes() {
        assert!(parse_outline(&json!([symbol("a"), flat_symbol()]), "abc").is_err());
        assert!(parse_outline(&json!([flat_symbol(), symbol("a")]), "abc").is_err());
        let mut nested = symbol("parent");
        nested["children"] = json!([flat_symbol()]);
        assert!(parse_outline(&json!([nested]), "abc").is_err());
        let mut hybrid = symbol("hybrid");
        hybrid["location"] = reference();
        assert!(parse_outline(&json!([hybrid]), "abc").is_err());
        for key in ["range", "selectionRange", "children", "detail"] {
            let mut hybrid = flat_symbol();
            hybrid[key] = Value::Null;
            assert!(parse_outline(&json!([hybrid]), "abc").is_err());
        }
        let mut hybrid = symbol("hybrid");
        hybrid["containerName"] = json!("container");
        assert!(parse_outline(&json!([hybrid]), "abc").is_err());
    }

    #[test]
    fn outline_rejects_malformed_required_and_optional_fields() {
        assert!(parse_outline(&symbol("x"), "abc").is_err());
        for value in [json!([null]), json!([true]), json!([[]])] {
            assert!(parse_outline(&value, "abc").is_err());
        }
        for key in ["name", "kind", "range", "selectionRange"] {
            let mut item = symbol("x");
            item.as_object_mut().unwrap().remove(key);
            assert!(parse_outline(&json!([item]), "abc").is_err());
        }
        for key in ["name", "detail", "children"] {
            for value in [Value::Null, json!(1), json!({})] {
                let mut item = symbol("x");
                item[key] = value;
                assert!(parse_outline(&json!([item]), "abc").is_err());
            }
        }
        for name in ["", " \t\n"] {
            assert!(parse_outline(&json!([symbol(name)]), "abc").is_err());
        }
        let mut item = flat_symbol();
        item["containerName"] = json!(8);
        assert!(parse_outline(&json!([item]), "abc").is_err());
    }

    #[test]
    fn unknown_unsigned_symbol_kinds_are_kept_but_malformed_kinds_fail() {
        for kind in [0, 27, u32::MAX] {
            let mut item = symbol("x");
            item["kind"] = json!(kind);
            assert_eq!(parse_outline(&json!([item]), "abc").unwrap()[0].kind, kind);
        }
        for kind in [
            Value::Null,
            json!(-1),
            json!(1.5),
            json!(1.0),
            json!("12"),
            json!(u64::from(u32::MAX) + 1),
        ] {
            let mut item = symbol("x");
            item["kind"] = kind;
            assert!(parse_outline(&json!([item]), "abc").is_err());
        }
    }

    #[test]
    fn hierarchical_selection_and_children_must_be_contained() {
        let mut item = symbol("x");
        item["selectionRange"] = range(2, 4);
        assert!(parse_outline(&json!([item]), "abcd").is_err());
        let mut item = symbol("x");
        item["range"] = range(1, 3);
        item["selectionRange"] = range(0, 2);
        assert!(parse_outline(&json!([item]), "abcd").is_err());
        let mut parent = symbol("parent");
        let mut child = symbol("child");
        child["range"] = range(0, 4);
        parent["children"] = json!([child]);
        assert!(parse_outline(&json!([parent]), "abcd").is_err());
        let mut item = symbol("x");
        item["selectionRange"] = range(3, 3);
        assert!(parse_outline(&json!([item]), "abc").is_ok());
    }

    #[test]
    fn hierarchical_ranges_use_strict_utf16_and_crlf_positions() {
        let source = "a😀b\r\né\rc\n";
        let mut item = symbol("unicode");
        item["range"] = json!({"start":{"line":0,"character":0},"end":{"line":3,"character":0}});
        item["selectionRange"] = range(1, 3);
        let rows = parse_outline(&json!([item.clone()]), source).unwrap();
        assert!(
            matches!(rows[0].location, OutlineLocation::Local { selection, .. } if selection.start.character == 1 && selection.end.character == 3)
        );
        for invalid in [
            json!({"line":0,"character":2}),
            json!({"line":0,"character":5}),
            json!({"line":1,"character":2}),
            json!({"line":4,"character":0}),
        ] {
            let mut bad = item.clone();
            bad["selectionRange"] = json!({"start":invalid.clone(),"end":invalid});
            assert!(parse_outline(&json!([bad]), source).is_err());
        }
        // A full range endpoint also must be valid, not just the selection.
        item["range"]["end"] = json!({"line":3,"character":1});
        assert!(parse_outline(&json!([item]), source).is_err());
    }

    #[test]
    fn empty_source_permits_only_zero_ranges() {
        let mut item = symbol("empty");
        item["range"] = range(0, 0);
        item["selectionRange"] = range(0, 0);
        assert!(parse_outline(&json!([item]), "").is_ok());
        assert!(parse_outline(&json!([symbol("x")]), "").is_err());
    }

    #[test]
    fn outline_count_limit_includes_all_descendants() {
        assert_eq!(
            parse_outline(&json!(vec![symbol("x"); MAX_OUTLINE_ITEMS]), "abc")
                .unwrap()
                .len(),
            MAX_OUTLINE_ITEMS
        );
        assert!(parse_outline(&json!(vec![symbol("x"); MAX_OUTLINE_ITEMS + 1]), "abc").is_err());
        let mut parent = symbol("parent");
        parent["children"] = json!(vec![symbol("child"); MAX_OUTLINE_ITEMS - 1]);
        assert_eq!(
            parse_outline(&json!([parent.clone()]), "abc")
                .unwrap()
                .len(),
            MAX_OUTLINE_ITEMS
        );
        assert!(parse_outline(&json!([parent, symbol("sibling")]), "abc").is_err());
    }

    fn nested(depth: usize) -> Value {
        let mut item = symbol("leaf");
        // An empty children list does not introduce an extra nesting level.
        item["children"] = json!([]);
        for _ in 0..depth {
            let mut parent = symbol("parent");
            parent["children"] = json!([item]);
            item = parent;
        }
        item
    }

    #[test]
    fn outline_depth_limit_is_inclusive_and_bounded() {
        let rows = parse_outline(&json!([nested(MAX_OUTLINE_DEPTH)]), "abc").unwrap();
        assert_eq!(rows.last().unwrap().depth, MAX_OUTLINE_DEPTH);
        assert!(parse_outline(&json!([nested(MAX_OUTLINE_DEPTH + 1)]), "abc").is_err());
        assert!(parse_outline(&json!([nested(128)]), "abc").is_err());
    }

    #[test]
    fn outline_text_budget_counts_names_details_containers_and_uris() {
        let mut item = symbol("n");
        item["detail"] = json!("d".repeat(MAX_RETAINED_BYTES - 1));
        assert!(parse_outline(&json!([item.clone()]), "abc").is_ok());
        item["detail"] = json!("d".repeat(MAX_RETAINED_BYTES));
        assert!(parse_outline(&json!([item]), "abc").is_err());
        let mut item = flat_symbol();
        item["name"] = json!("n");
        item["location"]["uri"] = json!("u".repeat(MAX_URI_BYTES));
        item["containerName"] = json!("c".repeat(MAX_RETAINED_BYTES - MAX_URI_BYTES - 1));
        assert!(parse_outline(&json!([item.clone()]), "").is_ok());
        item["name"] = json!("nn");
        assert!(parse_outline(&json!([item]), "").is_err());
        let mut item = symbol("x");
        item["name"] = json!("n".repeat(MAX_RETAINED_BYTES));
        assert!(parse_outline(&json!([item.clone()]), "abc").is_ok());
        assert!(parse_outline(&json!([item, symbol("extra")]), "abc").is_err());
    }

    #[test]
    fn text_limits_measure_utf8_bytes_not_scalar_count() {
        let mut item = reference();
        item["uri"] = json!("é".repeat(MAX_URI_BYTES / 2));
        assert!(parse_references(&json!([item.clone()])).is_ok());
        item["uri"] = json!("é".repeat(MAX_URI_BYTES / 2 + 1));
        assert!(parse_references(&json!([item])).is_err());
        let mut item = symbol("x");
        item["detail"] = json!("😀".repeat(MAX_RETAINED_BYTES / 4));
        assert!(parse_outline(&json!([item]), "abc").is_err());
    }

    #[test]
    fn batched_source_validation_matches_the_shared_strict_converter() {
        for source in ["", "abc", "a😀é中\r\né\rc\n", "\r\n\n\r", "😀😀😀"] {
            let mut valid = BTreeSet::new();
            for line in 0..8 {
                for character in 0..16 {
                    let position = Position { line, character };
                    let expected = position_to_offsets(source, position).is_ok();
                    assert_eq!(
                        validate_positions(source, &BTreeSet::from([position])).is_ok(),
                        expected,
                        "source {source:?}, position {position:?}"
                    );
                    if expected {
                        valid.insert(position);
                    }
                }
            }
            assert!(validate_positions(source, &valid).is_ok());
            // Sparse positions skip entire lines, and gaps within Unicode lines.
            let sparse = valid.into_iter().step_by(3).collect();
            assert!(validate_positions(source, &sparse).is_ok());
        }
    }

    #[test]
    fn large_source_with_many_distinct_boundaries_is_validated_in_a_batch() {
        let source = "😀éa".repeat(100_000);
        let values: Vec<_> = (0..MAX_OUTLINE_ITEMS)
            .map(|index| {
                let mut item = symbol("x");
                let start = index as u32 * 4;
                item["range"] = range(start, start + 4);
                item["selectionRange"] = range(start, start + 2);
                item
            })
            .collect();
        assert_eq!(
            parse_outline(&json!(values), &source).unwrap().len(),
            MAX_OUTLINE_ITEMS
        );
    }
}
