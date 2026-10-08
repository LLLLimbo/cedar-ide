//! Synthetic-only semantic witnesses for the shipping Quick Java acceptance.
//! No source, path, protocol response or arbitrary error enters its receipt.
use super::{marker_range, same_local_uri, CheckResult};
use crate::{language_results, text_edits};
use serde::Serialize;
use serde_json::Value;

pub(super) struct Fixture {
    pub path: &'static str,
    pub source: &'static str,
}

pub(super) const MAIN: Fixture = Fixture {
    path: "project with spaces 雪/src/CedarImportsDraft.java",
    source: "import java.util.Map;\nimport java.util.List;\nimport java.util.Set;\n\npublic class CedarImportsDraft {\n    Map<String, String> mapping;\n    List<String> listing;\n}\n",
};
pub(super) const DRAFT: &str = "import java.util.Map;\nimport java.util.List;\nimport java.util.Set;\n\npublic class CedarImportsDraft {\n    Map<String, String> mapping;\n    List<String> listing;\n    CedarUnsavedUnique unsavedOnly;\n}\n";
pub(super) const AMBIGUOUS: Fixture = Fixture {
    path: "project with spaces 雪/src/CedarImportsAmbiguous.java",
    source: "public class CedarImportsAmbiguous {\n    CedarSharedType unresolvedChoice;\n    CedarIndependentUnique independent;\n}\n",
};
pub(super) const LEFT: Fixture = Fixture {
    path: "project with spaces 雪/src/cedarimportfixture/left/CedarSharedType.java",
    source: "package cedarimportfixture.left;\npublic class CedarSharedType {}\n",
};
pub(super) const RIGHT: Fixture = Fixture {
    path: "project with spaces 雪/src/cedarimportfixture/right/CedarSharedType.java",
    source: "package cedarimportfixture.right;\npublic class CedarSharedType {}\n",
};
pub(super) const UNIQUE: Fixture = Fixture {
    path: "project with spaces 雪/src/cedarimportfixture/unique/CedarUnsavedUnique.java",
    source: "package cedarimportfixture.unique;\npublic class CedarUnsavedUnique {}\n",
};
pub(super) const INDEPENDENT: Fixture = Fixture {
    path: "project with spaces 雪/src/cedarimportfixture/unique/CedarIndependentUnique.java",
    source: "package cedarimportfixture.unique;\npublic class CedarIndependentUnique {}\n",
};
pub(super) const INDEX: Fixture = Fixture {
    path: "project with spaces 雪/src/CedarImportsIndex.java",
    source: "public class CedarImportsIndex {\n    cedarimportfixture.left.CedarSharedType left;\n    cedarimportfixture.right.CedarSharedType right;\n    cedarimportfixture.unique.CedarUnsavedUnique unique;\n    cedarimportfixture.unique.CedarIndependentUnique independent;\n}\n",
};
pub(super) const FIXTURES: [&Fixture; 7] = [
    &MAIN,
    &AMBIGUOUS,
    &LEFT,
    &RIGHT,
    &UNIQUE,
    &INDEPENDENT,
    &INDEX,
];

pub(super) fn exact_indexed_definition(
    value: &Value,
    uri: &str,
    fixture: &Fixture,
    type_name: &str,
) -> bool {
    language_results::parse_definitions(value).is_ok_and(|locations| {
        locations.len() == 1
            && same_local_uri(&locations[0].uri, uri)
            && locations[0].range == marker_range(fixture.source, type_name)
    })
}

/// Require the complete import set, unchanged declaration/body bytes, and the
/// retained java.util imports' order. Blank import-group lines are JDT-owned.
fn imports_and_body<'a>(text: &'a str, draft: &str) -> CheckResult<Vec<&'a str>> {
    let class = draft
        .find("public class ")
        .ok_or("synthetic class missing")?;
    let actual_class = text
        .find("public class ")
        .ok_or("organized class missing")?;
    if text[actual_class..] != draft[class..] {
        return Err("organize imports changed synthetic declaration or body bytes".into());
    }
    text[..actual_class]
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            if line.starts_with("import ") && line.ends_with(';') {
                Ok(line)
            } else {
                Err("organize imports returned unexpected prefix text".into())
            }
        })
        .collect()
}

pub(super) fn main_plan(value: &Value) -> CheckResult<text_edits::PlannedTextEdit> {
    let edits = text_edits::parse_text_edits(value)?;
    let plan = text_edits::plan_text_edits(DRAFT, &edits, 0)?;
    let imports = imports_and_body(&plan.text, DRAFT)?;
    let list = imports.iter().position(|s| *s == "import java.util.List;");
    let map = imports.iter().position(|s| *s == "import java.util.Map;");
    if plan.edit_count == 0
        || plan.text == DRAFT
        || imports.len() != 3
        || !matches!((list, map), (Some(a), Some(b)) if a < b)
        || imports
            .iter()
            .filter(|s| **s == "import cedarimportfixture.unique.CedarUnsavedUnique;")
            .count()
            != 1
    {
        return Err(
            "organize imports missed sort, unused removal or unique unsaved addition".into(),
        );
    }
    Ok(plan)
}

pub(super) fn ambiguity_plan(value: &Value) -> CheckResult<text_edits::PlannedTextEdit> {
    let edits = text_edits::parse_text_edits(value)?;
    let plan = text_edits::plan_text_edits(AMBIGUOUS.source, &edits, 0)?;
    let imports = imports_and_body(&plan.text, AMBIGUOUS.source)?;
    if plan.edit_count == 0
        || imports != ["import cedarimportfixture.unique.CedarIndependentUnique;"]
    {
        return Err(
            "organize imports silently chose an ambiguous type or missed independent addition"
                .into(),
        );
    }
    Ok(plan)
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    None,
    #[default]
    Setup,
    Support,
    IndexWitness,
    UnsavedSync,
    Organize,
    Preview,
    Cancel,
    Apply,
    Undo,
    Redo,
    Ambiguity,
    Close,
}

#[derive(Default, Serialize)]
pub(super) struct Evidence {
    pub kind: &'static str,
    pub exercised: bool,
    pub supported: bool,
    pub left_candidate_indexed: bool,
    pub right_candidate_indexed: bool,
    pub unsaved_type_indexed: bool,
    pub independent_type_indexed: bool,
    pub unsaved_version_acknowledged: bool,
    pub sorted_retained_imports: bool,
    pub unused_import_removed: bool,
    pub unsaved_unique_import_added: bool,
    pub preview_unchanged: bool,
    pub cancel_unchanged: bool,
    pub actual_frontend_apply: bool,
    pub one_undo_exact: bool,
    pub one_redo_exact: bool,
    pub draft_versions_synced: bool,
    pub ambiguous_candidates_skipped: bool,
    pub independent_import_added: bool,
    pub source_files_unchanged: bool,
    pub root_handle_signaled: bool,
    pub client_reaped: bool,
    pub synthetic_root_removed: bool,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
    pub main_edit_count: u16,
    pub ambiguity_edit_count: u16,
    pub observed_editor_stages: u8,
    pub failure_stage: Stage,
    pub elapsed_ms: u32,
    pub elapsed_saturated: bool,
}
impl Evidence {
    pub fn new() -> Self {
        Self {
            kind: "windows_java_organize_imports",
            ..Self::default()
        }
    }
    pub fn semantics_passed(&self) -> bool {
        self.exercised
            && self.supported
            && self.left_candidate_indexed
            && self.right_candidate_indexed
            && self.unsaved_type_indexed
            && self.independent_type_indexed
            && self.unsaved_version_acknowledged
            && self.sorted_retained_imports
            && self.unused_import_removed
            && self.unsaved_unique_import_added
            && self.preview_unchanged
            && self.cancel_unchanged
            && self.actual_frontend_apply
            && self.one_undo_exact
            && self.one_redo_exact
            && self.draft_versions_synced
            && self.ambiguous_candidates_skipped
            && self.independent_import_added
            && self.main_edit_count > 0
            && self.ambiguity_edit_count > 0
            && self.observed_editor_stages == 5
    }
}

#[test]
fn main_witness_requires_unsaved_addition_sort_removal_and_exact_body() {
    let prefix_end =
        super::completion::byte_to_position(DRAFT, DRAFT.find("public class ").unwrap()).unwrap();
    let response = |prefix: &str| serde_json::json!([{"range":{"start":{"line":0,"character":0},"end":{"line":prefix_end.line,"character":prefix_end.character}},"newText":prefix}]);
    let good = "import cedarimportfixture.unique.CedarUnsavedUnique;\n\nimport java.util.List;\nimport java.util.Map;\n\n";
    assert!(main_plan(&response(good)).is_ok());
    assert!(main_plan(&serde_json::json!([])).is_err());
    for wrong in [
        good.replace("import cedarimportfixture.unique.CedarUnsavedUnique;\n", ""),
        good.replace(
            "import java.util.List;\nimport java.util.Map;",
            "import java.util.Map;\nimport java.util.List;",
        ),
        good.replace(
            "import java.util.Map;",
            "import java.util.Map;\nimport java.util.Set;",
        ),
    ] {
        assert!(main_plan(&response(&wrong)).is_err());
    }
    let mut body_change = response(good);
    let body_range = marker_range(DRAFT, "unsavedOnly");
    body_change.as_array_mut().unwrap().push(serde_json::json!({"range":{"start":{"line":body_range.start.line,"character":body_range.start.character},"end":{"line":body_range.end.line,"character":body_range.end.character}},"newText":"changedBody"}));
    assert!(main_plan(&body_change).is_err());
    assert!(!MAIN.source.contains("CedarUnsavedUnique"));
    assert!(DRAFT.contains("CedarUnsavedUnique"));
}

#[test]
fn ambiguous_witness_requires_independent_addition_and_no_silent_candidate() {
    let response = |prefix: &str| serde_json::json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":prefix}]);
    let good = "import cedarimportfixture.unique.CedarIndependentUnique;\n\n";
    assert!(ambiguity_plan(&response(good)).is_ok());
    assert!(ambiguity_plan(&serde_json::json!([])).is_err());
    for candidate in ["left", "right"] {
        assert!(ambiguity_plan(&response(&format!(
            "{good}import cedarimportfixture.{candidate}.CedarSharedType;\n"
        )))
        .is_err());
    }
}

#[test]
fn index_witness_rejects_wrong_candidate_uri_range_and_empty_definition() {
    let uri = "file:///generated/left/CedarSharedType.java";
    let range = marker_range(LEFT.source, "CedarSharedType");
    let definition = serde_json::json!({"uri":uri,"range":{"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}}});
    assert!(exact_indexed_definition(
        &definition,
        uri,
        &LEFT,
        "CedarSharedType"
    ));
    assert!(!exact_indexed_definition(
        &definition,
        "file:///generated/right/CedarSharedType.java",
        &RIGHT,
        "CedarSharedType"
    ));
    assert!(!exact_indexed_definition(
        &serde_json::json!([]),
        uri,
        &LEFT,
        "CedarSharedType"
    ));
    let mut wrong = definition;
    wrong["range"]["start"]["character"] = serde_json::json!(0);
    assert!(!exact_indexed_definition(
        &wrong,
        uri,
        &LEFT,
        "CedarSharedType"
    ));
}

#[test]
fn organize_receipt_contains_only_fixed_scalars_and_no_fixture_content() {
    let value = serde_json::to_value(Evidence::new()).unwrap();
    let object = value.as_object().unwrap();
    for (key, value) in object {
        assert!(
            value.is_boolean()
                || value.is_u64()
                || matches!(key.as_str(), "kind" | "failure_stage")
        );
    }
    assert_eq!(value["kind"], "windows_java_organize_imports");
    assert_eq!(value["failure_stage"], "setup");
    assert!(!Evidence::new().semantics_passed());
    let encoded = value.to_string();
    for fixture in FIXTURES {
        assert!(!encoded.contains(fixture.path));
        assert!(!encoded.contains(fixture.source));
    }
}
