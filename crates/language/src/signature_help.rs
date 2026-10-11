//! Bounded, inert signature help shared by the agent and desktop. Documentation
//! and extension fields are never retained or interpreted.
use crate::Error;
use serde::Serialize;
use serde_json::{Map, Value};
use std::ops::Range;

pub const MAX_SIGNATURES: usize = 32;
pub const MAX_PARAMETERS: usize = 64;
pub const MAX_LABEL_BYTES: usize = 8192;
pub const MAX_RETAINED_LABEL_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ParameterLabel {
    Text(String),
    Offsets([u32; 2]),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Parameter {
    pub label: ParameterLabel,
    /// UTF-8 byte boundaries in the signature label, suitable for plain text UI.
    /// Ambiguous string labels and zero-length offsets have no visible range.
    #[serde(skip)]
    pub highlight: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub label: String,
    pub parameters: Vec<Parameter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_parameter: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelp {
    pub signatures: Vec<Signature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_signature: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_parameter: Option<u32>,
}

impl SignatureHelp {
    pub fn server_selected_signature(&self) -> Option<usize> {
        self.active_signature
            .map(|index| index as usize)
            .filter(|index| *index < self.signatures.len())
    }

    pub fn initial_signature(&self) -> Option<usize> {
        self.server_selected_signature()
            .or_else(|| (!self.signatures.is_empty()).then_some(0))
    }

    /// A signature's own integer hint takes precedence even when unusable.
    /// The top-level hint only describes the initially displayed signature.
    /// Deliberately unlike LSP's default, absent hints never invent parameter 0.
    pub fn active_parameter(&self, signature_index: usize) -> Option<usize> {
        let signature = self.signatures.get(signature_index)?;
        signature
            .active_parameter
            .or_else(|| {
                (self.initial_signature() == Some(signature_index))
                    .then_some(self.active_parameter)
                    .flatten()
            })
            .map(|index| index as usize)
            .filter(|index| *index < signature.parameters.len())
    }

    pub fn highlight(&self, signature_index: usize) -> Option<Range<usize>> {
        let parameter_index = self.active_parameter(signature_index)?;
        self.signatures
            .get(signature_index)?
            .parameters
            .get(parameter_index)?
            .highlight
            .clone()
    }
}

fn invalid(message: &str) -> Error {
    Error::Protocol(format!("Invalid signature help result: {message}"))
}

fn object(value: &Value) -> Result<&Map<String, Value>, Error> {
    value
        .as_object()
        .ok_or_else(|| invalid("expected an object"))
}

fn unsigned(value: &Value) -> Result<u32, Error> {
    value
        .as_u64()
        .filter(|value| *value <= i32::MAX as u64)
        .map(|value| value as u32)
        .ok_or_else(|| invalid("hints and offsets must be unsigned 31-bit integers"))
}

fn hint(object: &Map<String, Value>, field: &str) -> Result<Option<u32>, Error> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => unsigned(value).map(Some),
    }
}

fn label<'a>(value: &'a Value, retained: &mut usize) -> Result<&'a str, Error> {
    let label = value
        .as_str()
        .filter(|label| label.len() <= MAX_LABEL_BYTES)
        .ok_or_else(|| invalid("labels must be strings of at most 8192 UTF-8 bytes"))?;
    // Check both limits before any label text is copied into the typed model.
    *retained = retained
        .checked_add(label.len())
        .filter(|bytes| *bytes <= MAX_RETAINED_LABEL_BYTES)
        .ok_or_else(|| invalid("retained labels exceed 128 KiB"))?;
    Ok(label)
}

fn byte_boundary(label: &str, offset: u32) -> Result<usize, Error> {
    let mut units = 0u32;
    for (byte, character) in label.char_indices() {
        if units == offset {
            return Ok(byte);
        }
        units += character.len_utf16() as u32;
        if units > offset {
            return Err(invalid("offset splits a UTF-16 surrogate pair"));
        }
    }
    if units == offset {
        Ok(label.len())
    } else {
        Err(invalid("offset exceeds the signature label"))
    }
}

fn parameter(value: &Value, signature: &str, retained: &mut usize) -> Result<Parameter, Error> {
    let parameter = object(value)?;
    let value = parameter
        .get("label")
        .ok_or_else(|| invalid("parameter label is required"))?;
    if value.is_string() {
        let text = label(value, retained)?;
        if text.is_empty() {
            return Err(invalid("string parameter labels cannot be empty"));
        }
        let start = signature
            .find(text)
            .ok_or_else(|| invalid("parameter label is absent from the signature"))?;
        // Check overlapping occurrences too, rather than str::match_indices.
        let next = start + signature[start..].chars().next().unwrap().len_utf8();
        let unique = !signature[next..].contains(text);
        return Ok(Parameter {
            label: ParameterLabel::Text(text.to_owned()),
            highlight: unique.then_some(start..start + text.len()),
        });
    }
    let offsets = value
        .as_array()
        .filter(|offsets| offsets.len() == 2)
        .ok_or_else(|| invalid("parameter labels must be strings or exactly two offsets"))?;
    let start = unsigned(&offsets[0])?;
    let end = unsigned(&offsets[1])?;
    if start > end {
        return Err(invalid("parameter offsets are reversed"));
    }
    let start_byte = byte_boundary(signature, start)?;
    let end_byte = byte_boundary(signature, end)?;
    Ok(Parameter {
        label: ParameterLabel::Offsets([start, end]),
        highlight: (start_byte < end_byte).then_some(start_byte..end_byte),
    })
}

/// Parse an LSP SignatureHelp or null, rejecting the entire result on malformed
/// rows. Numeric hints outside their arrays remain present but unusable.
pub fn parse(value: &Value) -> Result<SignatureHelp, Error> {
    if value.is_null() {
        return Ok(SignatureHelp::default());
    }
    let help = object(value)?;
    let signatures = help
        .get("signatures")
        .and_then(Value::as_array)
        .filter(|signatures| signatures.len() <= MAX_SIGNATURES)
        .ok_or_else(|| invalid("expected at most 32 signatures"))?;
    let active_signature = hint(help, "activeSignature")?;
    let active_parameter = hint(help, "activeParameter")?;
    let mut retained = 0usize;
    let mut result = Vec::with_capacity(signatures.len());
    for value in signatures {
        let signature = object(value)?;
        let text = label(
            signature
                .get("label")
                .ok_or_else(|| invalid("signature label is required"))?,
            &mut retained,
        )?;
        let active_parameter = hint(signature, "activeParameter")?;
        let parameters = match signature.get("parameters") {
            None => &[][..],
            Some(value) => value
                .as_array()
                .filter(|parameters| parameters.len() <= MAX_PARAMETERS)
                .map(Vec::as_slice)
                .ok_or_else(|| invalid("expected at most 64 parameters"))?,
        };
        let mut parsed = Vec::with_capacity(parameters.len());
        for value in parameters {
            parsed.push(parameter(value, text, &mut retained)?);
        }
        result.push(Signature {
            label: text.to_owned(),
            parameters: parsed,
            active_parameter,
        });
    }
    Ok(SignatureHelp {
        signatures: result,
        active_signature,
        active_parameter,
    })
}

pub(crate) fn normalize_result(value: Value) -> Result<Value, Error> {
    serde_json::to_value(parse(&value)?)
        .map_err(|error| invalid(&format!("cannot encode normalized result: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signature() -> Value {
        json!({"label":"f(雪, 🦀)","parameters":[{"label":"雪"},{"label":[5,7]}]})
    }

    #[test]
    fn preserves_only_bounded_inert_labels_parameters_and_hints() {
        let source = json!({
            "signatures":[{
                "label":"f(<b>x</b>)", "documentation":{"kind":"markdown","value":"[run](command:evil)"},
                "activeParameter":0, "command":{"command":"evil"},
                "parameters":[{"label":"<b>x</b>","documentation":"secret","activeParameter":-1}]
            }], "activeSignature":0, "data":{"deep":[1,2,3]}, "activeParameter":1
        });
        let expected = json!({"signatures":[{"label":"f(<b>x</b>)","parameters":[{"label":"<b>x</b>"}],"activeParameter":0}],"activeSignature":0,"activeParameter":1});
        assert_eq!(normalize_result(source.clone()).unwrap(), expected);
        assert_eq!(parse(&source).unwrap(), parse(&expected).unwrap());
        assert_eq!(parse(&source).unwrap().highlight(0), Some(2..10));
        assert_eq!(
            normalize_result(Value::Null).unwrap(),
            json!({"signatures":[]})
        );
        assert_eq!(parse(&Value::Null).unwrap().initial_signature(), None);
        assert_eq!(
            parse(&json!({"signatures":[{"label":"f()"}]}))
                .unwrap()
                .signatures[0]
                .parameters
                .len(),
            0
        );
    }

    #[test]
    fn utf16_offsets_are_scalar_boundaries_and_ranges_are_utf8() {
        let help = parse(&json!({"signatures":[signature()],"activeParameter":1})).unwrap();
        assert_eq!(help.highlight(0), Some(7..11));
        assert_eq!(&help.signatures[0].label[help.highlight(0).unwrap()], "🦀");
        for range in [json!([5, 5]), json!([7, 7]), json!([8, 8])] {
            let mut row = signature();
            row["parameters"] = json!([{"label":range}]);
            assert_eq!(
                parse(&json!({"signatures":[row],"activeParameter":0}))
                    .unwrap()
                    .highlight(0),
                None
            );
        }
        for range in [
            json!([6, 7]),
            json!([5, 6]),
            json!([8, 9]),
            json!([7, 5]),
            json!([-1, 3]),
            json!([0, 2147483648_u64]),
            json!([0, 1.5]),
            json!([0, "1"]),
            json!([0]),
            json!([0, 1, 2]),
        ] {
            let mut row = signature();
            row["parameters"] = json!([{"label":range}]);
            assert!(parse(&json!({"signatures":[row]})).is_err());
        }
    }

    #[test]
    fn string_labels_require_a_match_and_ambiguous_overlapping_matches_do_not_highlight() {
        for (label, parameter, expected) in [
            ("f(雪)", "雪", Some(2..5)),
            ("f(x, x)", "x", None),
            ("aaa", "aa", None),
            ("🦀🦀🦀", "🦀🦀", None),
        ] {
            let help = parse(&json!({"signatures":[{"label":label,"parameters":[{"label":parameter}]}],"activeParameter":0})).unwrap();
            assert_eq!(help.highlight(0), expected);
        }
        for parameter in ["", "missing"] {
            assert!(parse(
                &json!({"signatures":[{"label":"f(x)","parameters":[{"label":parameter}]}]})
            )
            .is_err());
        }
    }

    #[test]
    fn hints_keep_own_precedence_without_invented_defaults_or_cross_overload_fallback() {
        let mut value =
            json!({"signatures":[signature(),signature()],"activeSignature":1,"activeParameter":1});
        let help = parse(&value).unwrap();
        assert_eq!(help.server_selected_signature(), Some(1));
        assert_eq!(help.initial_signature(), Some(1));
        assert_eq!(help.active_parameter(0), None);
        assert_eq!(help.active_parameter(1), Some(1));
        value["signatures"][1]["activeParameter"] = json!(0);
        assert_eq!(parse(&value).unwrap().active_parameter(1), Some(0));
        value["signatures"][1]["activeParameter"] = json!(99);
        let help = parse(&value).unwrap();
        assert_eq!(help.signatures[1].active_parameter, Some(99));
        assert_eq!(help.active_parameter(1), None);
        assert_eq!(help.highlight(1), None);
        value["signatures"][1]["activeParameter"] = Value::Null;
        assert_eq!(parse(&value).unwrap().active_parameter(1), Some(1));
        value["activeSignature"] = json!(99);
        let help = parse(&value).unwrap();
        assert_eq!(help.active_signature, Some(99));
        assert_eq!(help.server_selected_signature(), None);
        assert_eq!(help.initial_signature(), Some(0));
        assert_eq!(help.active_parameter(0), Some(1));
        assert_eq!(help.active_parameter(1), None);
        value["activeParameter"] = json!(99);
        assert_eq!(parse(&value).unwrap().active_parameter(0), None);
        value["activeParameter"] = Value::Null;
        let help = parse(&value).unwrap();
        assert_eq!(help.active_parameter(0), None);
        assert_eq!(help.active_parameter(1), None);
        assert_eq!(help.active_parameter(usize::MAX), None);
        for hint in [Value::Null, json!(2147483647)] {
            value["activeSignature"] = hint;
            assert_eq!(parse(&value).unwrap().server_selected_signature(), None);
        }
    }

    #[test]
    fn rejects_malformed_hints_and_rows_atomically_but_ignores_extensions() {
        for hint in [
            json!(false),
            json!("1"),
            json!(-1),
            json!(1.5),
            json!(2147483648_u64),
            json!([]),
            json!({}),
        ] {
            for pointer in [
                "/activeSignature",
                "/activeParameter",
                "/signatures/0/activeParameter",
            ] {
                let mut value =
                    json!({"signatures":[signature()],"activeSignature":0,"activeParameter":0});
                value["signatures"][0]["activeParameter"] = json!(0);
                *value.pointer_mut(pointer).unwrap() = hint.clone();
                assert!(parse(&value).is_err(), "{value}");
            }
        }
        for bad in [
            json!(false),
            json!([]),
            json!({}),
            json!({"label":null}),
            json!({"label":"f()","parameters":null}),
            json!({"label":"f()","parameters":{}}),
            json!({"label":"f()","parameters":[null]}),
            json!({"label":"f()","parameters":[{}]}),
            json!({"label":"f()","parameters":[{"label":true}]}),
        ] {
            assert!(parse(&json!({"signatures":[signature(),bad]})).is_err());
        }
        for bad in [
            json!(false),
            json!([]),
            json!({}),
            json!({"signatures":null}),
            json!({"signatures":{}}),
        ] {
            assert!(parse(&bad).is_err());
        }
    }

    #[test]
    fn exact_count_and_utf8_byte_limits_are_enforced_before_copying_labels() {
        assert!(parse(&json!({"signatures":vec![signature();MAX_SIGNATURES]})).is_ok());
        assert!(parse(&json!({"signatures":vec![signature();MAX_SIGNATURES+1]})).is_err());
        for (count, allowed) in [(MAX_PARAMETERS, true), (MAX_PARAMETERS + 1, false)] {
            let value = json!({"signatures":[{"label":"f(x)","parameters":vec![json!({"label":[2,3]});count]}]});
            assert_eq!(parse(&value).is_ok(), allowed);
        }
        let at_limit = "🦀".repeat(MAX_LABEL_BYTES / 4);
        assert_eq!(at_limit.len(), MAX_LABEL_BYTES);
        assert!(parse(&json!({"signatures":[{"label":at_limit}]})).is_ok());
        assert!(parse(&json!({"signatures":[{"label":format!("{at_limit}x")}]})).is_err());
        let row = json!({"label":at_limit});
        assert!(parse(&json!({"signatures":vec![row.clone();16]})).is_ok());
        assert!(parse(&json!({"signatures":vec![row;17]})).is_err());
        // Parameter string copies count as retained labels too.
        let row = json!({"label":at_limit,"parameters":[{"label":at_limit}]});
        assert!(parse(&json!({"signatures":vec![row.clone();8]})).is_ok());
        assert!(parse(&json!({"signatures":vec![row;9]})).is_err());
    }
}
