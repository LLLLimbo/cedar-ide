//! Strict plain implementation locations. The URI is inert data, never a grant
//! to read or open a file. Navigation must resolve it through the workspace.
use crate::Error;
use serde_json::{Map, Value};

const MAX_LOCATIONS: usize = 1024;
const MAX_URI_BYTES: usize = 16 * 1024;
const MAX_RETAINED_BYTES: usize = 512 * 1024;

fn invalid(message: &str) -> Error {
    Error::Protocol(format!("Invalid implementation result: {message}"))
}

fn exact_object<'a>(value: &'a Value, fields: &[&str]) -> Result<&'a Map<String, Value>, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("expected a plain location object"))?;
    if object.len() != fields.len() || object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(invalid("missing or unsupported location fields"));
    }
    Ok(object)
}

fn position(value: &Value) -> Result<(u64, u64), Error> {
    let object = exact_object(value, &["line", "character"])?;
    let number = |key| {
        object[key]
            .as_u64()
            .filter(|number| *number <= i32::MAX as u64)
            .ok_or_else(|| invalid("positions must be unsigned 31-bit integers"))
    };
    Ok((number("line")?, number("character")?))
}

fn uri(value: &Value) -> Result<&str, Error> {
    let uri = value
        .as_str()
        .filter(|uri| !uri.is_empty() && uri.len() <= MAX_URI_BYTES)
        .ok_or_else(|| invalid("location URI must contain 1..16384 UTF-8 bytes"))?;
    let Some((scheme, rest)) = uri.split_once(':') else {
        return Err(invalid("location URI must be absolute"));
    };
    if rest.is_empty()
        || !scheme
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        || !scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
        || uri.chars().any(|ch| {
            ch.is_control()
                || ch.is_whitespace()
                || matches!(ch, '\\' | '"' | '<' | '>' | '^' | '`' | '{' | '|' | '}')
        })
    {
        return Err(invalid(
            "location URI must be absolute without whitespace or controls",
        ));
    }
    // Do not silently normalize malformed escapes or hidden control bytes.
    // Escaped spaces, raw Unicode and opaque JDT URIs remain inert display data.
    let mut decoded = Vec::with_capacity(uri.len());
    let mut bytes = uri.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let high = bytes.next().and_then(|byte| (byte as char).to_digit(16));
            let low = bytes.next().and_then(|byte| (byte as char).to_digit(16));
            match (high, low) {
                (Some(high), Some(low)) => ((high << 4) | low) as u8,
                _ => return Err(invalid("location URI has a malformed percent escape")),
            }
        } else {
            byte
        };
        decoded.push(byte);
    }
    let decoded =
        std::str::from_utf8(&decoded).map_err(|_| invalid("location URI has invalid UTF-8"))?;
    if decoded.chars().any(char::is_control) {
        return Err(invalid(
            "location URI has invalid UTF-8 or encoded controls",
        ));
    }
    Ok(uri)
}

pub(crate) fn normalize_result(value: Value) -> Result<Value, Error> {
    let locations = match value {
        Value::Null => return Ok(Value::Array(vec![])),
        Value::Array(locations) => locations,
        value @ Value::Object(_) => vec![value],
        _ => return Err(invalid("expected null, Location or Location[]")),
    };
    if locations.len() > MAX_LOCATIONS {
        return Err(invalid("response exceeds the 1024-location limit"));
    }
    let mut retained = 0usize;
    for location in &locations {
        let location = exact_object(location, &["uri", "range"])?;
        retained = retained
            .checked_add(uri(&location["uri"])?.len())
            .filter(|bytes| *bytes <= MAX_RETAINED_BYTES)
            .ok_or_else(|| invalid("retained URI text exceeds the 512 KiB limit"))?;
        let range = exact_object(&location["range"], &["start", "end"])?;
        if position(&range["start"])? > position(&range["end"])? {
            return Err(invalid("location range is reversed"));
        }
    }
    // Strict fields, fixed nesting, bounded row count and bounded URI text
    // bound retained data; the transport independently applies its frame cap.
    // A malformed later row rejects the whole response, never partial success.
    Ok(Value::Array(locations))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn location(uri: &str) -> Value {
        json!({"uri":uri,"range":{"start":{"line":2,"character":3},"end":{"line":2,"character":7}}})
    }

    #[test]
    fn normalizes_only_plain_bounded_unversioned_locations() {
        assert_eq!(normalize_result(Value::Null).unwrap(), json!([]));
        assert_eq!(normalize_result(json!([])).unwrap(), json!([]));
        for uri in [
            "file:///C:/src/Hello%20雪.java",
            "file:/C:/src/A.java",
            "jdt://contents/library/Type.class",
            "file:///outside/Type.java",
        ] {
            let entry = location(uri);
            assert_eq!(normalize_result(entry.clone()).unwrap(), json!([entry]));
        }
        let rows = json!(vec![location("file:///a.java"); MAX_LOCATIONS]);
        assert_eq!(normalize_result(rows.clone()).unwrap(), rows);
        let uri_at_limit = format!("file:///{}", "a".repeat(MAX_URI_BYTES - 8));
        assert_eq!(uri_at_limit.len(), MAX_URI_BYTES);
        assert!(normalize_result(json!(vec![location(&uri_at_limit); 32])).is_ok());
        assert!(normalize_result(json!(vec![location(&uri_at_limit); 33])).is_err());
        assert!(normalize_result(location(&(uri_at_limit + "a"))).is_err());
    }

    #[test]
    fn rejects_malformed_links_extensions_and_oversized_results_atomically() {
        let good = location("file:///a.java");
        let mut cases = vec![
            json!(false),
            json!(3),
            json!("file:///a.java"),
            json!({}),
            json!({"targetUri":"file:///a.java","targetRange":good["range"],"targetSelectionRange":good["range"]}),
            json!({"kind":"create","uri":"file:///a.java"}),
            json!({"changes":{"file:///a.java":[]}}),
            json!([good.clone(), null]),
            json!(vec![good.clone(); MAX_LOCATIONS + 1]),
        ];
        for field in [
            "targetUri",
            "command",
            "data",
            "version",
            "newText",
            "resourceOperations",
        ] {
            let mut entry = good.clone();
            entry[field] = json!({"unexpected":[1, 2, 3]});
            cases.push(entry);
        }
        for bad in [
            json!(-1),
            json!(2147483648_u64),
            json!(1.5),
            json!("0"),
            Value::Null,
        ] {
            let mut entry = good.clone();
            entry["range"]["start"]["line"] = bad;
            cases.push(entry);
        }
        for pointer in ["/range", "/range/start", "/range/end"] {
            let mut entry = good.clone();
            entry.pointer_mut(pointer).unwrap()["extra"] = json!(true);
            cases.push(entry);
        }
        let mut reversed = good.clone();
        reversed["range"]["end"]["line"] = json!(1);
        cases.push(reversed);
        for uri in [
            "",
            "relative.java",
            "1file:/a",
            "file:",
            "file:///a b",
            "file:///a\\b",
            "file:///a\n",
            "file:///a%",
            "file:///a%GG",
            "file:///a%00",
            "file:///a%0a",
            "file:///a%C2%85",
            "file:///a%FF",
        ] {
            cases.push(location(uri));
        }
        for bad in cases {
            assert!(normalize_result(bad.clone()).is_err(), "{bad}");
            if !bad.is_array() {
                assert!(normalize_result(json!([good, bad])).is_err());
            }
        }
    }
}
