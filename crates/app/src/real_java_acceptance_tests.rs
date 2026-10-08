//! Shared, pure acceptance predicates and the real headless editor transaction.
//! Native Windows orchestration is feature-gated separately below.
#![allow(dead_code)]

use crate::{completion, editor_state, language_results, model::Document};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::Cell;

pub(super) const SOURCE: &str = "public class Main {\n    public static void main(String[] args) {\n        String greeting = \"Hello Cedar\";\n        System.out.println(greeting);\n        int broken = \"oops\";\n    }\n}\n";
pub(super) const PROJECT_DIR: &str = "project with spaces 雪";
pub(super) const SOURCE_PATH: &str = "project with spaces 雪/src/Main.java";
pub(super) const INITIAL_DATA_DIR: &str = "jdt data initial 雪";
pub(super) const RESTART_DATA_DIR: &str = "jdt data restart 雪";
pub(super) type CheckResult<T> = Result<T, String>;

pub(super) fn validate_fixture_layout(
    root: &std::path::Path,
    data: &std::path::Path,
) -> CheckResult<()> {
    let project = root.join(PROJECT_DIR);
    // Eclipse forbids project locations that contain its workspace (-data).
    // Both remain inside the synthetic agent root, as distinct sibling areas.
    if data.parent() != Some(root)
        || data.starts_with(&project)
        || project.starts_with(data)
        || root.join(SOURCE_PATH) != project.join("src/Main.java")
    {
        return Err("synthetic Java project and JDT data must be distinct siblings".into());
    }
    Ok(())
}

pub(super) fn corrected_source() -> String {
    SOURCE.replace("int broken = \"oops\";", "int correctedOnly = 42;")
}

pub(super) fn marker_range(source: &str, marker: &str) -> completion::Range {
    let start = source.find(marker).expect("synthetic source marker");
    completion::Range {
        start: completion::byte_to_position(source, start).unwrap(),
        end: completion::byte_to_position(source, start + marker.len()).unwrap(),
    }
}

// JDT legitimately returns file:/ with raw Unicode. Compare decoded local path
// bytes, rejecting authorities, query/fragment data, malformed escapes and NUL.
pub(super) fn same_local_uri(actual: &str, expected: &str) -> bool {
    fn path(uri: &str) -> Option<Vec<u8>> {
        let path = uri.strip_prefix("file:")?;
        let path = path.strip_prefix("//").unwrap_or(path);
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#', '\\']) {
            return None;
        }
        let mut decoded = Vec::new();
        let mut bytes = path.bytes();
        while let Some(byte) = bytes.next() {
            decoded.push(if byte == b'%' {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                ((high << 4) | low) as u8
            } else {
                byte
            });
        }
        if decoded.contains(&0) {
            return None;
        }
        if decoded.get(2) == Some(&b':') && decoded.get(1).is_some_and(u8::is_ascii_alphabetic) {
            decoded[1].make_ascii_uppercase();
        }
        Some(decoded)
    }
    matches!((path(actual), path(expected)), (Some(a), Some(b)) if a == b)
}

pub(super) fn diagnostics_match(value: &Value, uri: &str, version: i32, corrected: bool) -> bool {
    let Some(actual_uri) = value["uri"].as_str() else {
        return false;
    };
    if !same_local_uri(actual_uri, uri) {
        return false;
    }
    let mut parsed = language_results::Diagnostics::default();
    if parsed.apply(value).is_err() {
        return false;
    }
    let Some(batch) = parsed.files.get(actual_uri) else {
        return false;
    };
    if batch.version.is_some_and(|v| v != version) {
        return false;
    }
    if corrected {
        !batch.items.iter().any(|d| d.severity == 1)
            && batch.items.iter().any(|d| {
                d.severity == 2
                    && d.message.contains("correctedOnly")
                    && d.message.contains("not used")
                    && d.range == marker_range(&corrected_source(), "correctedOnly")
            })
    } else {
        batch.items.iter().any(|d| {
            d.severity == 1
                && d.message.contains("cannot convert from String to int")
                && d.range == marker_range(SOURCE, "\"oops\"")
        })
    }
}

pub(super) fn exact_definition(value: &Value, uri: &str) -> CheckResult<String> {
    let locations = language_results::parse_definitions(value)?;
    if locations.len() != 1
        || !same_local_uri(&locations[0].uri, uri)
        || locations[0].range != marker_range(SOURCE, "greeting")
    {
        return Err("definition did not identify the exact source declaration".into());
    }
    Ok(locations[0].uri.clone())
}

/// Exercise CedarApp's actual resolve acceptance and Document Undo/Redo. The
/// callback synchronizes these exact buffers, never an Undoer clone or inferred
/// replacement text. No OS window, Client capability bypass or trust action.
pub(super) fn editor_transaction(
    original: Value,
    resolved: Value,
    stage: &Cell<FailureStage>,
    mut synchronized: impl FnMut(i32, &Document) -> CheckResult<()>,
) -> CheckResult<()> {
    use crate::language_ui::{Action, ActionKind, QueryContext};
    stage.set(FailureStage::Apply);
    crate::language_ui::validate_resolved_identity(&original, &resolved)?;
    let byte = SOURCE.rfind("greeting").ok_or("missing reference")? + 3;
    let cursor = completion::byte_to_position(SOURCE, byte)?;
    let before_cursor = SOURCE[..byte].chars().count();
    let applied = completion::apply_completion(SOURCE, cursor, &resolved)?;
    if applied.edit_count != 2
        || !applied.skipped_advisory
        || !applied
            .text
            .starts_with("import java.util.GregorianCalendar;\n")
        || !applied
            .text
            .contains("System.out.println(GregorianCalendar);")
    {
        return Err("completion was not an atomic primary edit plus deferred import".into());
    }
    let mut app = crate::CedarApp::empty();
    app.language.running = true;
    app.language.session = 1;
    app.language.acceptance_sequence = 1;
    let mut doc = Document::new(
        1,
        SOURCE_PATH.into(),
        SOURCE.into(),
        "synthetic-original".into(),
    );
    doc.cursor = crate::model::cursor_location(SOURCE, before_cursor);
    let mut state = editor_state::load(&app.editor_ctx, &mut doc);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(before_cursor),
        )));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    app.documents.push(doc);
    app.active_document = Some(1);
    app.apply_language_action(
        Action {
            session: 1,
            kind: ActionKind::ResolveCompletion {
                context: QueryContext {
                    session: 1,
                    document: 1,
                    edit_version: 0,
                    source: SOURCE.into(),
                    cursor,
                },
                original,
                acceptance: 1,
            },
        },
        resolved,
    );
    if let Some(error) = app.error.take() {
        return Err(error);
    }
    check_document(&mut app, &applied.text, applied.cursor_chars, 1, true)?;
    stage.set(FailureStage::Sync);
    synchronized(2, &app.documents[0])?;
    stage.set(FailureStage::Undo);
    history_key(&mut app, false);
    check_document(&mut app, SOURCE, before_cursor, 2, false)?;
    stage.set(FailureStage::Sync);
    synchronized(3, &app.documents[0])?;
    stage.set(FailureStage::Redo);
    history_key(&mut app, true);
    check_document(&mut app, &applied.text, applied.cursor_chars, 3, true)?;
    stage.set(FailureStage::Sync);
    synchronized(4, &app.documents[0])?;
    Ok(())
}

fn check_document(
    app: &mut crate::CedarApp,
    expected_text: &str,
    expected_cursor: usize,
    version: u64,
    dirty: bool,
) -> CheckResult<()> {
    let doc = &mut app.documents[0];
    let state = editor_state::load(&app.editor_ctx, doc);
    let expected_range = egui::text::CCursorRange::one(egui::text::CCursor::new(expected_cursor));
    if doc.text != expected_text
        || doc.cursor != crate::model::cursor_location(expected_text, expected_cursor)
        || state.cursor.char_range() != Some(expected_range)
        || doc.edit_version != version
        || doc.dirty() != dirty
        || doc.saved_text != SOURCE
        || doc.revision.as_deref() != Some("synthetic-original")
    {
        return Err("actual Document text/cursor/version/dirty/history invariant failed".into());
    }
    Ok(())
}

fn history_key(app: &mut crate::CedarApp, redo: bool) {
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if redo {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                },
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[0]);
        },
    );
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum AgentEvidence {
    #[serde(rename = "agent_java_started")]
    Started {
        session: u32,
        pid: u32,
        creation_time_100ns_since_1601: u64,
    },
    #[serde(rename = "agent_java_stopped")]
    Stopped {
        session: u32,
        jdk_symbol_verified: bool,
        shutdown_api_succeeded: bool,
        root_handle_signaled: bool,
        root_exit_code: Option<u32>,
        gracefully_exited: bool,
        shutdown_elapsed_ms: u64,
    },
}

pub(super) fn parse_evidence(bytes: &[u8]) -> CheckResult<Vec<AgentEvidence>> {
    if bytes.len() > 32 * 1024 || !bytes.ends_with(b"\n") {
        return Err("invalid bounded Java lifecycle evidence".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "non-UTF-8 lifecycle evidence")?;
    let lines: Vec<_> = text.lines().collect();
    if lines.is_empty() || lines.len() > 32 {
        return Err("invalid lifecycle record count".into());
    }
    lines
        .into_iter()
        .map(|line| {
            serde_json::from_str(line).map_err(|_| "invalid typed Java lifecycle record".into())
        })
        .collect()
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionMode {
    Initial,
    FreshData,
    ReusedData,
}

#[derive(Serialize)]
pub(super) struct SessionEvidence {
    kind: &'static str,
    pub session: u32,
    mode: SessionMode,
    pub initialization_ms: u64,
    pub semantic_checks_passed: bool,
    pub exact_diagnostics: bool,
    pub exact_definition: bool,
    pub real_completion: bool,
    pub deferred_import_resolve: bool,
    pub primary_identity_unchanged: bool,
    pub two_atomic_edits: bool,
    pub advisory_command_skipped: bool,
    pub actual_undo: bool,
    pub actual_redo: bool,
    pub versions_2_3_4_synced: bool,
    pub correction_diagnostics: bool,
    pub source_unchanged: bool,
    pub root_observed_live: bool,
    pub root_identity_verified: bool,
    pub jdk_symbol_verified: bool,
    pub shutdown_api_succeeded: bool,
    pub root_handle_signaled: bool,
    pub root_exit_code: Option<u32>,
    pub gracefully_exited: bool,
    pub shutdown_elapsed_ms: u64,
}
impl SessionEvidence {
    pub fn new(session: u32, mode: SessionMode) -> Self {
        Self {
            kind: "windows_java_session",
            session,
            mode,
            initialization_ms: 0,
            semantic_checks_passed: false,
            exact_diagnostics: false,
            exact_definition: false,
            real_completion: false,
            deferred_import_resolve: false,
            primary_identity_unchanged: false,
            two_atomic_edits: false,
            advisory_command_skipped: false,
            actual_undo: false,
            actual_redo: false,
            versions_2_3_4_synced: false,
            correction_diagnostics: false,
            source_unchanged: false,
            root_observed_live: false,
            root_identity_verified: false,
            jdk_symbol_verified: false,
            shutdown_api_succeeded: false,
            root_handle_signaled: false,
            root_exit_code: None,
            gracefully_exited: false,
            shutdown_elapsed_ms: 0,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureStage {
    None,
    Setup,
    Initialize,
    Open,
    Diagnostics,
    Hover,
    Definition,
    Completion,
    Resolve,
    Apply,
    Undo,
    Redo,
    Sync,
    Correction,
    Close,
    Stop,
    RootExit,
    AgentExit,
    FixtureCleanup,
}

#[derive(Serialize)]
pub(super) struct CleanupEvidence {
    pub kind: &'static str,
    pub sessions_completed: u32,
    pub agent_exit_zero: bool,
    pub source_unchanged: bool,
    pub observed_roots_exited: bool,
    pub synthetic_root_removed: bool,
    pub failure_stage: FailureStage,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
}

#[cfg(windows)]
#[path = "windows_java_agent_tests.rs"]
mod windows;

#[test]
fn generated_java_layout_keeps_project_and_server_data_separate() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    for name in [INITIAL_DATA_DIR, RESTART_DATA_DIR] {
        let data = root.join(name);
        validate_fixture_layout(root, &data).unwrap();
        assert!(!root.join(SOURCE_PATH).starts_with(&data));
    }
    for invalid in [
        root.to_path_buf(),
        root.join(PROJECT_DIR),
        root.join(PROJECT_DIR).join(INITIAL_DATA_DIR),
    ] {
        assert!(validate_fixture_layout(root, &invalid).is_err());
    }
}

#[test]
fn exact_java_witnesses_reject_unrelated_stale_and_empty_results() {
    const URI: &str = "file:///C:/fixture%20%E9%9B%AA/src/Main.java";
    let range = |range: completion::Range| json!({"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}});
    let mut initial = json!({"uri":URI,"version":1,"diagnostics":[{"severity":1,"message":"Type mismatch: cannot convert from String to int","range":range(marker_range(SOURCE,"\"oops\""))}]});
    assert!(diagnostics_match(&initial, URI, 1, false));
    initial["version"] = json!(2);
    assert!(!diagnostics_match(&initial, URI, 1, false));
    initial["version"] = Value::Null;
    initial["diagnostics"][0]["range"]["start"]["character"] = json!(0);
    assert!(!diagnostics_match(&initial, URI, 1, false));
    let mut corrected = json!({"uri":URI,"diagnostics":[{"severity":2,"message":"The value of the local variable correctedOnly is not used","range":range(marker_range(&corrected_source(),"correctedOnly"))}]});
    assert!(diagnostics_match(&corrected, URI, 5, true));
    corrected["diagnostics"] = json!([]);
    assert!(!diagnostics_match(&corrected, URI, 5, true));
    let definition = json!({"uri":"file:/c:/fixture%20雪/src/Main.java","range":range(marker_range(SOURCE,"greeting"))});
    assert!(exact_definition(&definition, URI).is_ok());
    assert!(exact_definition(&definition, "file:///C:/other/Main.java").is_err());
    for invalid in [
        "file://host/C:/fixture%20雪/src/Main.java",
        "file:///C:/fixture%20雪/src/Main.java#fragment",
        "file:///C:/fixture%20雪/src/Main.java%00",
        "file:///C:/fixture%ZZ雪/src/Main.java",
    ] {
        assert!(!same_local_uri(invalid, URI));
    }
}

#[test]
fn real_jdt_capture_drives_actual_document_history_and_versions() {
    let captured: Value = serde_json::from_str(
        include_str!("../../language/tests/evidence/jdtls-1.61.0-completion-resolve.jsonl").trim(),
    )
    .unwrap();
    let mut versions = Vec::new();
    editor_transaction(
        captured["input"].clone(),
        captured["payload"].clone(),
        &Cell::new(FailureStage::Apply),
        |version, doc| {
            versions.push((version, doc.edit_version, doc.dirty()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(versions, [(2, 1, true), (3, 2, false), (4, 3, true)]);
    let mut changed = captured["payload"].clone();
    changed["textEdit"]["newText"] = json!("different");
    let mut called = false;
    assert!(editor_transaction(
        captured["input"].clone(),
        changed,
        &Cell::new(FailureStage::Apply),
        |_, _| {
            called = true;
            Ok(())
        }
    )
    .is_err());
    assert!(
        !called,
        "identity replacement must be rejected before buffer synchronization"
    );
}

#[test]
fn lifecycle_evidence_is_typed_bounded_and_requires_complete_lines() {
    let line = b"{\"kind\":\"agent_java_started\",\"session\":1,\"pid\":7,\"creation_time_100ns_since_1601\":1}\n";
    assert!(parse_evidence(line).is_ok());
    assert!(parse_evidence(&line[..line.len() - 1]).is_err());
    assert!(parse_evidence(&line.repeat(33)).is_err());
    assert!(parse_evidence(b"{\"kind\":\"private_raw_protocol\"}\n").is_err());
    assert!(parse_evidence(b"{\"kind\":\"agent_java_started\",\"session\":1,\"pid\":7,\"creation_time_100ns_since_1601\":1,\"raw\":\"private\"}\n").is_err());
}
