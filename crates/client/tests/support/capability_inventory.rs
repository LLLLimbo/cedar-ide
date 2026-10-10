//! Independent complete inventories for the normal isolated shipping agent.
use cedar_protocol::AgentInfo;

pub fn assert_shipping_inventory(info: &AgentInfo) {
    info.validate().unwrap();
    let mut expected = vec![
        "list",
        "read",
        "write",
        "search",
        "language_open",
        "language_change",
        "language_close",
        "language_events",
        "language_stop",
        "language_query",
        "language_format",
        "language_references",
        "language_document_symbols",
        "language_workspace_symbols",
        "language_resolve_uri",
        "language_resolve_completion",
    ];
    if matches!(info.os.as_str(), "linux" | "macos" | "windows") {
        expected.extend([
            "run_start",
            "run_poll",
            "run_cancel",
            "git_changes",
            "git_diff",
        ]);
    }
    if info.os != "windows" {
        expected.push("language_start");
    }
    if matches!(info.os.as_str(), "linux" | "macos") {
        expected.extend(["run", "git_status"]);
    }
    if matches!(info.os.as_str(), "linux" | "windows") {
        expected.extend([
            "language_start_java",
            "language_start_java_begin",
            "language_start_java_poll",
            "language_start_java_cancel",
            "java_diagnostics_refresh",
            "language_organize_java_imports",
            "language_java_implementations",
        ]);
    }
    let maven = [
        "language_start_java_maven_begin",
        "language_maven_model",
        "language_maven_dependencies",
    ];
    if info.os == "windows" {
        expected.extend(maven);
    }
    expected.sort_unstable();
    assert!(expected.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(info.capabilities, expected);
    if matches!(info.os.as_str(), "linux" | "windows") {
        assert_eq!(info.capabilities.len(), 31);
    }
    let wire = serde_json::to_value(info).unwrap();
    if info.os == "linux" {
        let groups = ["java_maven_dependencies_v1", "java_maven_leaf_v1"];
        assert_eq!(info.capability_groups, groups);
        assert_eq!(wire["capability_groups"], serde_json::json!(groups));
        for capability in maven {
            assert!(!info.capabilities.iter().any(|name| name == capability));
            assert!(info.supports(capability));
        }
    } else {
        assert!(info.capability_groups.is_empty());
        assert!(wire.get("capability_groups").is_none());
        for capability in maven {
            assert_eq!(info.supports(capability), info.os == "windows");
        }
    }
}
