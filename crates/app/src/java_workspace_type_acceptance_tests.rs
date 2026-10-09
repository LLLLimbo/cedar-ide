//! One finite unopened-type witness for the shipping Quick Java route.
//! Only fixed booleans, stages and bounded elapsed time leave this fixture.
use super::{marker_range, same_local_uri, CheckResult, PROJECT_DIR};
use crate::{completion, language_results};
use serde::Serialize;
use serde_json::Value;

pub(super) const PACKAGE: &str = "cedarworkspacefixture";

pub(super) struct Fixture {
    pub name: String,
    pub negative_query: String,
    pub path: String,
    pub source: String,
}

impl Fixture {
    pub fn new(nonce: u128) -> Self {
        let name = format!("CedarUnopenedType{nonce:X}");
        Self {
            negative_query: format!("CedarAbsentType{nonce:X}"),
            path: format!("{PROJECT_DIR}/src/{PACKAGE}/{name}.java"),
            // Non-BMP text before the identifier makes the declaration witness
            // distinguish UTF-16 positions from byte/scalar offsets.
            source: format!("package {PACKAGE};\n/* 雪😀 */ public class {name} {{}}\n"),
            name,
        }
    }

    pub fn declaration(&self) -> completion::Range {
        marker_range(&self.source, &self.name)
    }

    pub fn exact_symbol(&self, value: &Value, expected_uri: &str) -> CheckResult<String> {
        let rows = value
            .as_array()
            .ok_or("workspace type result is not an array")?;
        if rows.len() != 1
            || rows[0]["name"] != self.name
            || rows[0]["kind"] != 5
            || rows[0]["containerName"] != PACKAGE
        {
            return Err("workspace type result did not identify the exact unique class".into());
        }
        // JDT LS v1.61.0 WorkspaceSymbolHandler uses JDTUtils.toLocation(type)
        // for source types; that overload defaults to LocationType.NAME_RANGE.
        let locations = language_results::parse_definitions(&rows[0]["location"])?;
        if locations.len() != 1
            || !same_local_uri(&locations[0].uri, expected_uri)
            || locations[0].range != self.declaration()
        {
            return Err(
                "workspace type location did not match the exact source declaration".into(),
            );
        }
        Ok(locations[0].uri.clone())
    }
}

pub(super) fn empty_result(value: &Value) -> bool {
    value.is_null() || value.as_array().is_some_and(Vec::is_empty)
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    None,
    #[default]
    Setup,
    Support,
    Query,
    NegativeQuery,
    Resolve,
    Read,
    Frontend,
}

#[derive(Default, Serialize)]
pub(super) struct Evidence {
    pub kind: &'static str,
    pub exercised: bool,
    pub capability_supported: bool,
    pub provider_supported: bool,
    pub target_unopened: bool,
    pub exact_type_name: bool,
    pub exact_type_uri: bool,
    pub exact_declaration_range: bool,
    pub negative_query_empty: bool,
    pub resolved_path_exact: bool,
    pub ordinary_read_exact: bool,
    pub actual_frontend_navigation: bool,
    pub dirty_buffer_reused: bool,
    pub undo_redo_preserved: bool,
    pub source_unchanged: bool,
    pub root_handle_signaled: bool,
    pub client_reaped: bool,
    pub synthetic_root_removed: bool,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
    pub failure_stage: Stage,
    pub elapsed_ms: u32,
    pub elapsed_saturated: bool,
}

impl Evidence {
    pub fn new() -> Self {
        Self {
            kind: "windows_java_workspace_types",
            ..Self::default()
        }
    }

    pub fn semantics_passed(&self) -> bool {
        self.exercised
            && self.capability_supported
            && self.provider_supported
            && self.target_unopened
            && self.exact_type_name
            && self.exact_type_uri
            && self.exact_declaration_range
            && self.negative_query_empty
            && self.resolved_path_exact
            && self.ordinary_read_exact
            && self.actual_frontend_navigation
            && self.dirty_buffer_reused
            && self.undo_redo_preserved
    }
}

#[test]
fn exact_unopened_type_requires_its_name_container_uri_and_utf16_declaration() {
    let fixture = Fixture::new(0x1234);
    let uri = "file:///C:/workspace%20%E9%9B%AA/Owned.java";
    let range = fixture.declaration();
    let value = serde_json::json!([{
        "name": fixture.name, "kind": 5, "containerName": PACKAGE,
        "location": {"uri": uri, "range": {
            "start": {"line": range.start.line, "character": range.start.character},
            "end": {"line": range.end.line, "character": range.end.character}
        }}
    }]);
    assert_eq!(fixture.exact_symbol(&value, uri).unwrap(), uri);
    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.character, 23);
    assert!(fixture
        .exact_symbol(&value, "file:///C:/other.java")
        .is_err());
    for (pointer, invalid) in [
        ("/0/name", serde_json::json!("WrongType")),
        ("/0/kind", serde_json::json!(6)),
        ("/0/containerName", serde_json::json!("wrong.package")),
        ("/0/location/range/start/character", serde_json::json!(22)),
        ("/0/location/range/end/character", serde_json::json!(24)),
        ("/0/location/range", Value::Null),
    ] {
        let mut invalid_result = value.clone();
        *invalid_result.pointer_mut(pointer).unwrap() = invalid;
        assert!(fixture.exact_symbol(&invalid_result, uri).is_err());
    }
    assert!(fixture.exact_symbol(&serde_json::json!([]), uri).is_err());
    assert!(fixture
        .exact_symbol(&serde_json::json!([value[0], value[0]]), uri)
        .is_err());
    assert!(empty_result(&Value::Null));
    assert!(empty_result(&serde_json::json!([])));
    assert!(!empty_result(&value));
    assert!(!empty_result(&serde_json::json!({"items": []})));
}

#[test]
fn workspace_type_receipt_has_no_query_path_source_or_protocol_data() {
    let fixture = Fixture::new(0x1234);
    let value = serde_json::to_value(Evidence::new()).unwrap();
    for (key, value) in value.as_object().unwrap() {
        assert!(
            value.is_boolean()
                || value.is_u64()
                || matches!(key.as_str(), "kind" | "failure_stage")
        );
    }
    assert_eq!(value["kind"], "windows_java_workspace_types");
    assert_eq!(value["failure_stage"], "setup");
    assert!(!Evidence::new().semantics_passed());
    let encoded = value.to_string();
    for private in [
        &fixture.name,
        &fixture.negative_query,
        &fixture.path,
        &fixture.source,
    ] {
        assert!(!encoded.contains(private));
    }
}
