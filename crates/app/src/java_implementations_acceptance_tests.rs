//! Finite implementation witnesses for the shipping Quick Java route.
//! Only fixed booleans, stages and bounded counts/time leave this fixture.
use super::{marker_range, same_local_uri, CheckResult, PROJECT_DIR};
use crate::{completion, language_navigation_results};
use serde::Serialize;
use serde_json::Value;

pub(super) const PACKAGE: &str = "cedarimplementationfixture";

pub(super) struct Source {
    pub name: String,
    pub path: String,
    pub text: String,
}

pub(super) struct Fixture {
    pub interface: Source,
    pub concrete: Source,
    pub inherited: Source,
    pub method: String,
    pub negative: String,
}

impl Fixture {
    pub fn new(nonce: u128) -> Self {
        let interface = format!("CedarContract{nonce:X}");
        let concrete = format!("CedarConcrete{nonce:X}");
        let inherited = format!("CedarInherited{nonce:X}");
        let method = format!("cedarImplementation{nonce:X}");
        let negative = format!("CedarUnimplemented{nonce:X}");
        let source = |name: String, text: String| Source {
            path: format!("{PROJECT_DIR}/src/{PACKAGE}/{name}.java"),
            name,
            text,
        };
        Self {
            interface: source(interface.clone(), format!(
                "package {PACKAGE};\n/* 雪😀 */ public interface {interface} {{\n    /* 雪😀 */ void {method}();\n}}\n/* 雪😀 */ interface {negative} {{}}\n"
            )),
            concrete: source(concrete.clone(), format!(
                "package {PACKAGE};\n/* 雪😀 */ public class {concrete} implements {interface} {{\n    /* 雪😀 */ public void {method}() {{}}\n}}\n"
            )),
            inherited: source(inherited.clone(), format!(
                "package {PACKAGE};\n/* 雪😀 */ public class {inherited} extends {concrete} {{}}\n"
            )),
            method,
            negative,
        }
    }

    pub fn sources(&self) -> [&Source; 3] {
        [&self.interface, &self.concrete, &self.inherited]
    }

    pub fn type_cursor(&self) -> completion::Position {
        marker_range(&self.interface.text, &self.interface.name).start
    }

    pub fn method_cursor(&self) -> completion::Position {
        marker_range(&self.interface.text, &self.method).start
    }

    pub fn negative_cursor(&self) -> completion::Position {
        marker_range(&self.interface.text, &self.negative).start
    }

    pub fn exact_types(
        &self,
        value: &Value,
        concrete_uri: &str,
        inherited_uri: &str,
    ) -> CheckResult<String> {
        let locations = language_navigation_results::parse_java_implementations(value)?;
        if locations.len() != 2 {
            return Err("implementation type result did not contain exactly two subtypes".into());
        }
        let expected = [
            (
                concrete_uri,
                marker_range(&self.concrete.text, &self.concrete.name),
            ),
            (
                inherited_uri,
                marker_range(&self.inherited.text, &self.inherited.name),
            ),
        ];
        for (uri, range) in expected {
            if locations
                .iter()
                .filter(|row| same_local_uri(&row.uri, uri) && row.range == range)
                .count()
                != 1
            {
                return Err(
                    "implementation type result missed an exact source URI/name range".into(),
                );
            }
        }
        Ok(locations
            .iter()
            .find(|row| same_local_uri(&row.uri, concrete_uri))
            .unwrap()
            .uri
            .clone())
    }

    pub fn exact_method(&self, value: &Value, concrete_uri: &str) -> CheckResult<()> {
        let locations = language_navigation_results::parse_java_implementations(value)?;
        if locations.len() != 1
            || !same_local_uri(&locations[0].uri, concrete_uri)
            || locations[0].range != marker_range(&self.concrete.text, &self.method)
        {
            return Err(
                "implementation method result was not the sole concrete declaration".into(),
            );
        }
        Ok(())
    }
}

pub(super) fn result_count(value: &Value) -> CheckResult<u16> {
    let count = language_navigation_results::parse_java_implementations(value)?.len();
    if count > 128 {
        return Err("implementation result count exceeded its finite evidence bound".into());
    }
    Ok(count as u16)
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    None,
    #[default]
    Setup,
    Support,
    Open,
    TypeQuery,
    MethodQuery,
    NegativeQuery,
    Resolve,
    Read,
    Frontend,
    Close,
}

#[derive(Default, Serialize)]
pub(super) struct Evidence {
    pub kind: &'static str,
    pub exercised: bool,
    pub capability_supported: bool,
    pub provider_supported: bool,
    pub query_version_acknowledged: bool,
    pub targets_unopened: bool,
    pub exact_type_uris: bool,
    pub exact_type_ranges: bool,
    pub exact_method_uri: bool,
    pub exact_method_range: bool,
    pub inherited_method_absent: bool,
    pub negative_query_empty: bool,
    pub utf16_ranges_exact: bool,
    pub resolved_path_exact: bool,
    pub ordinary_read_exact: bool,
    pub actual_frontend_navigation: bool,
    pub full_selection_preserved: bool,
    pub dirty_buffer_reused: bool,
    pub undo_redo_preserved: bool,
    pub retained_context_preserved: bool,
    pub source_files_unchanged: bool,
    pub root_handle_signaled: bool,
    pub client_reaped: bool,
    pub synthetic_root_removed: bool,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
    pub type_result_count: u16,
    pub method_result_count: u16,
    pub negative_result_count: u16,
    pub failure_stage: Stage,
    pub elapsed_ms: u32,
    pub elapsed_saturated: bool,
}

impl Evidence {
    pub fn new() -> Self {
        Self {
            kind: "windows_java_implementations",
            ..Self::default()
        }
    }

    pub fn semantics_passed(&self) -> bool {
        self.exercised
            && self.capability_supported
            && self.provider_supported
            && self.query_version_acknowledged
            && self.targets_unopened
            && self.exact_type_uris
            && self.exact_type_ranges
            && self.exact_method_uri
            && self.exact_method_range
            && self.inherited_method_absent
            && self.negative_query_empty
            && self.utf16_ranges_exact
            && self.resolved_path_exact
            && self.ordinary_read_exact
            && self.actual_frontend_navigation
            && self.full_selection_preserved
            && self.dirty_buffer_reused
            && self.undo_redo_preserved
            && self.retained_context_preserved
            && self.type_result_count == 2
            && self.method_result_count == 1
            && self.negative_result_count == 0
    }
}

#[test]
fn exact_implementation_witness_rejects_missing_extra_fabricated_or_wrong_locations() {
    let fixture = Fixture::new(0x1234);
    let concrete_uri = "file:///C:/owned%20%E9%9B%AA/Concrete.java";
    let inherited_uri = "file:///C:/owned%20%E9%9B%AA/Inherited.java";
    let location = |uri: &str, range: completion::Range| {
        serde_json::json!({"uri":uri,"range":{
            "start":{"line":range.start.line,"character":range.start.character},
            "end":{"line":range.end.line,"character":range.end.character}
        }})
    };
    let concrete = location(
        concrete_uri,
        marker_range(&fixture.concrete.text, &fixture.concrete.name),
    );
    let inherited = location(
        inherited_uri,
        marker_range(&fixture.inherited.text, &fixture.inherited.name),
    );
    let types = serde_json::json!([concrete, inherited]);
    assert_eq!(
        fixture
            .exact_types(&types, concrete_uri, inherited_uri)
            .unwrap(),
        concrete_uri
    );
    assert!(fixture
        .exact_types(
            &serde_json::json!([inherited, concrete]),
            concrete_uri,
            inherited_uri
        )
        .is_ok());
    for wrong in [
        Value::Null,
        serde_json::json!([]),
        serde_json::json!([concrete]),
        serde_json::json!([concrete, concrete]),
        serde_json::json!([concrete, inherited, inherited]),
    ] {
        assert!(fixture
            .exact_types(&wrong, concrete_uri, inherited_uri)
            .is_err());
    }
    for (pointer, wrong) in [
        ("/0/uri", serde_json::json!("file:///C:/other.java")),
        ("/0/range/start/character", serde_json::json!(22)),
        ("/1/range/end/character", serde_json::json!(24)),
    ] {
        let mut altered = types.clone();
        *altered.pointer_mut(pointer).unwrap() = wrong;
        assert!(fixture
            .exact_types(&altered, concrete_uri, inherited_uri)
            .is_err());
    }
    let mut extended = types.clone();
    extended[0]["command"] = serde_json::json!("synthetic forbidden extension");
    assert!(fixture
        .exact_types(&extended, concrete_uri, inherited_uri)
        .is_err());
    let linked = serde_json::json!([{
        "targetUri": concrete_uri, "targetRange": concrete["range"],
        "targetSelectionRange": concrete["range"]
    }, inherited]);
    assert!(fixture
        .exact_types(&linked, concrete_uri, inherited_uri)
        .is_err());
    let method = location(
        concrete_uri,
        marker_range(&fixture.concrete.text, &fixture.method),
    );
    assert!(fixture
        .exact_method(&serde_json::json!([method]), concrete_uri)
        .is_ok());
    let fabricated = location(
        inherited_uri,
        marker_range(&fixture.concrete.text, &fixture.method),
    );
    for wrong in [
        Value::Null,
        serde_json::json!([]),
        serde_json::json!([fabricated]),
        serde_json::json!([method, fabricated]),
        serde_json::json!([method, method]),
        types,
    ] {
        assert!(fixture.exact_method(&wrong, concrete_uri).is_err());
    }
    assert_eq!(
        marker_range(&fixture.concrete.text, &fixture.concrete.name)
            .start
            .character,
        23
    );
    assert_eq!(
        marker_range(&fixture.concrete.text, &fixture.method)
            .start
            .character,
        26
    );
    assert!(!fixture.inherited.text.contains(&fixture.method));
    // Native backend normalization must have occurred before this witness.
    assert!(result_count(&Value::Null).is_err());
    assert_eq!(result_count(&serde_json::json!([])).unwrap(), 0);
    assert!(result_count(&serde_json::json!({"items":[]})).is_err());
}

#[test]
fn implementation_receipt_contains_only_fixed_scalars_without_fixture_content() {
    let fixture = Fixture::new(0x1234);
    let value = serde_json::to_value(Evidence::new()).unwrap();
    for (key, value) in value.as_object().unwrap() {
        assert!(
            value.is_boolean()
                || value.is_u64()
                || matches!(key.as_str(), "kind" | "failure_stage")
        );
    }
    assert_eq!(value["kind"], "windows_java_implementations");
    assert_eq!(value["failure_stage"], "setup");
    assert!(!Evidence::new().semantics_passed());
    let encoded = value.to_string();
    for source in fixture.sources() {
        for private in [&source.name, &source.path, &source.text] {
            assert!(!encoded.contains(private));
        }
    }
    assert!(!encoded.contains(&fixture.method));
    assert!(!encoded.contains(&fixture.negative));
}
