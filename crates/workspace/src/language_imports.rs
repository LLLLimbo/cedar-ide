//! The only executable command exposed by this bridge is a preview for one
//! acknowledged Java document. A returned URI never chooses a filesystem path.
use super::{error, java_diagnostics_refresh_supported, lsp_error, Workspace};
use cedar_protocol::{Payload, RemoteError, MAX_FILE_BYTES};
use serde_json::{json, Map, Value};

const COMMAND: &str = "java.edit.organizeImports";
const MAX_EDITS: usize = 1024;
const MAX_URI_BYTES: usize = 16 * 1024;

pub(super) fn supported(production_java: bool, initialize: &Value) -> bool {
    // The exact Standard release is the same audited identity as refresh; the
    // command additionally requires an explicit static advertisement. Server
    // metadata is compatibility evidence, not binary authentication.
    java_diagnostics_refresh_supported(production_java, initialize)
        && initialize["capabilities"]["executeCommandProvider"]["commands"]
            .as_array()
            .is_some_and(|commands| {
                commands.iter().all(Value::is_string)
                    && commands
                        .iter()
                        .any(|command| command.as_str() == Some(COMMAND))
            })
}

impl Workspace {
    pub(super) fn organize_java_imports(
        &self,
        path: &str,
        version: i32,
    ) -> Result<Payload, RemoteError> {
        let uri = self.language_uri(path)?;
        let session = self
            .language
            .as_ref()
            .ok_or_else(|| error("language_not_running", "Start a language server first"))?;
        if !session.production_java || !session.java_organize_imports {
            return Err(error(
                "language_organize_imports_unsupported",
                "Organize Imports requires a supported Standard JDT session started with the Java route and its advertised command",
            ));
        }
        if version <= 0 {
            return Err(error(
                "invalid_version",
                "Organize Imports requires a positive synchronized document version",
            ));
        }
        let document = session.open_document(&uri)?;
        if !document.is_java {
            return Err(error(
                "invalid_language",
                "Organize Imports requires a .java source document synchronized as Java",
            ));
        }
        document.require_version(version)?;
        // Never forward caller command/arguments, folders, source, or returned
        // commands. No save, applyEdit, retry, or automatic application occurs.
        let value = session
            .client
            .request(
                "workspace/executeCommand",
                json!({"command":COMMAND,"arguments":[uri]}),
            )
            .map_err(lsp_error)?;
        let value = normalize_workspace_edit(value, &uri, cfg!(windows))?;
        Ok(Payload::Language { value })
    }
}

fn invalid(message: &str) -> RemoteError {
    error("language_imports_invalid_edit", message)
}

fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
) -> Result<&'a Map<String, Value>, RemoteError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("Organize Imports edit must be an object"))?;
    if object.len() != fields.len() || object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(invalid(
            "Organize Imports edit contains missing or unsupported fields",
        ));
    }
    Ok(object)
}

fn position(value: &Value) -> Result<(u32, u32), RemoteError> {
    let object = exact_object(value, &["line", "character"])?;
    let integer = |field| {
        object[field]
            .as_u64()
            .filter(|number| *number <= i32::MAX as u64)
            .map(|number| number as u32)
            .ok_or_else(|| invalid("Organize Imports positions must be unsigned 31-bit integers"))
    };
    Ok((integer("line")?, integer("character")?))
}

fn validate_text_edits(value: &Value) -> Result<(), RemoteError> {
    let edits = value
        .as_array()
        .ok_or_else(|| invalid("Organize Imports changes must contain a plain TextEdit array"))?;
    if edits.len() > MAX_EDITS {
        return Err(invalid("Organize Imports exceeds the 1024-edit limit"));
    }
    let mut bytes = 0usize;
    for edit in edits {
        let edit = exact_object(edit, &["range", "newText"])?;
        let range = exact_object(&edit["range"], &["start", "end"])?;
        if position(&range["start"])? > position(&range["end"])? {
            return Err(invalid("Organize Imports contains a reversed range"));
        }
        let text = edit["newText"]
            .as_str()
            .ok_or_else(|| invalid("Organize Imports newText must be a string"))?;
        if text.contains('\0') {
            return Err(invalid("Organize Imports insertion contains NUL"));
        }
        bytes = bytes
            .checked_add(text.len())
            .filter(|bytes| *bytes <= MAX_FILE_BYTES)
            .ok_or_else(|| invalid("Organize Imports insertions exceed the 1 MiB limit"))?;
    }
    // The frontend binds this whole array to the captured source and checks
    // actual UTF-16 boundaries, overlapping ranges and final size before preview.
    Ok(())
}

/// Accept only a local file URI spelling of the one agent-generated URI. This
/// comparison neither consults the filesystem nor authorizes any other opened
/// document. JDT can return file:/C:/... and raw UTF-8 for file:///C:/... .
fn current_document_uri(actual: &str, expected: &str, windows: bool) -> bool {
    let Some(actual) = strict_local_uri_path(actual, windows) else {
        return false;
    };
    strict_local_uri_path(expected, windows).is_some_and(|expected| expected == actual)
}

fn strict_local_uri_path(uri: &str, windows: bool) -> Option<Vec<u8>> {
    if uri.len() > MAX_URI_BYTES {
        return None;
    }
    // Only no-authority file:/ and file:/// forms. Do not let a URL parser
    // normalize dot segments, separators, authority, query or fragment first.
    let path = uri.strip_prefix("file:")?;
    let path = path.strip_prefix("//").unwrap_or(path);
    if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#', '\\']) {
        return None;
    }
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            let byte = ((high << 4) | low) as u8;
            if matches!(byte, b'/' | b'\\') {
                return None;
            }
            byte
        } else {
            // JDT can emit raw Unicode, but ASCII spaces stay percent-escaped.
            if byte == b' ' {
                return None;
            }
            byte
        };
        if byte.is_ascii_control() {
            return None;
        }
        decoded.push(byte);
    }
    let text = std::str::from_utf8(&decoded).ok()?;
    if text.chars().any(char::is_control)
        || text[1..]
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    if windows {
        if decoded.len() < 5 || !decoded[1].is_ascii_alphabetic() || decoded[2..4] != *b":/" {
            return None;
        }
        for part in text[4..].split('/') {
            if part.ends_with(['.', ' ']) || part.contains([':', '<', '>', '|', '?', '*']) {
                return None;
            }
            let stem = part.split('.').next()?.to_ascii_uppercase();
            if matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            }) {
                return None;
            }
        }
        decoded[1].make_ascii_uppercase();
    }
    Some(decoded)
}

fn normalize_workspace_edit(value: Value, uri: &str, windows: bool) -> Result<Value, RemoteError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("Organize Imports must return a changes-only WorkspaceEdit"))?;
    if object.keys().any(|key| key != "changes") {
        return Err(invalid(
            "Organize Imports returned unsupported workspace edit fields",
        ));
    }
    let Some(changes) = object.get("changes") else {
        return Ok(json!([]));
    };
    let changes = changes
        .as_object()
        .ok_or_else(|| invalid("Organize Imports changes must be an object"))?;
    if changes.len() > 1 {
        return Err(invalid(
            "Organize Imports returned edits for multiple documents",
        ));
    }
    let Some((actual, edits)) = changes.iter().next() else {
        return Ok(json!([]));
    };
    if !current_document_uri(actual, uri, windows) {
        return Err(invalid(
            "Organize Imports returned an invalid URI or edits for a different document",
        ));
    }
    validate_text_edits(edits)?;
    // Move only after the entire envelope and array have passed validation.
    let Value::Object(mut object) = value else {
        unreachable!()
    };
    let Value::Object(changes) = object.remove("changes").unwrap() else {
        unreachable!()
    };
    Ok(changes.into_values().next().unwrap())
}

#[cfg(test)]
#[path = "language_imports_tests.rs"]
mod tests;
