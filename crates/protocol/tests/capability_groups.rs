use cedar_protocol::{
    capability_group_members, read_frame, supports_capability, write_frame, AgentInfo, Payload,
    Response, AGENT_INFO_SCHEMA, JAVA_LANGUAGE_SESSION_CAPABILITIES, JAVA_MAVEN_CAPABILITIES,
    JAVA_MAVEN_DEPENDENCIES_CAPABILITY, JAVA_MAVEN_DEPENDENCIES_GROUP, JAVA_MAVEN_LEAF_GROUP,
    JAVA_STARTUP_CAPABILITIES, MAX_AGENT_CAPABILITIES, MAX_AGENT_CAPABILITY_GROUPS,
    MAX_CAPABILITY_GROUPS_IDENTIFIER_BYTES, MAX_CAPABILITY_GROUPS_JSON_BYTES,
    MAX_CAPABILITY_GROUP_BYTES, PROTOCOL_VERSION,
};
use serde_json::{json, Value};

// The existing Linux 0.37 flat advertisement, kept literal so compatibility
// cannot accidentally depend on the new group expansion or backend selection.
const LEGACY_31_CAPABILITIES: &[&str] = &[
    "git_changes",
    "git_diff",
    "git_status",
    "java_diagnostics_refresh",
    "language_change",
    "language_close",
    "language_document_symbols",
    "language_events",
    "language_format",
    "language_java_implementations",
    "language_open",
    "language_organize_java_imports",
    "language_query",
    "language_references",
    "language_resolve_completion",
    "language_resolve_uri",
    "language_start",
    "language_start_java",
    "language_start_java_begin",
    "language_start_java_cancel",
    "language_start_java_poll",
    "language_stop",
    "language_workspace_symbols",
    "list",
    "read",
    "run",
    "run_cancel",
    "run_poll",
    "run_start",
    "search",
    "write",
];

fn info(groups: &[&str]) -> AgentInfo {
    AgentInfo {
        schema: AGENT_INFO_SCHEMA,
        version: "0.38.0-fixture".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        capabilities: Vec::new(),
        capability_groups: groups.iter().map(|group| (*group).into()).collect(),
    }
}

fn hello(info: AgentInfo) -> Response {
    Response {
        id: 38,
        result: Ok(Payload::Hello {
            protocol: PROTOCOL_VERSION,
            root: "/fixture".into(),
            agent: Some(info),
        }),
    }
}

#[test]
fn groups_are_optional_and_empty_groups_keep_the_old_wire_shape() {
    let original = info(&[]);
    let encoded = serde_json::to_value(&original).unwrap();
    assert!(encoded.get("capability_groups").is_none());
    let decoded: AgentInfo = serde_json::from_value(encoded.clone()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, original);

    let mut explicit_empty = encoded.clone();
    explicit_empty["capability_groups"] = json!([]);
    let decoded: AgentInfo = serde_json::from_value(explicit_empty).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, original);
    assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    assert_eq!(PROTOCOL_VERSION, 4);
    assert_eq!(AGENT_INFO_SCHEMA, 1);
    assert_eq!(MAX_AGENT_CAPABILITIES, 32);
}

#[test]
fn exact_known_groups_claim_only_the_existing_maven_operations() {
    assert_eq!(JAVA_MAVEN_LEAF_GROUP, "java_maven_leaf_v1");
    assert_eq!(JAVA_MAVEN_DEPENDENCIES_GROUP, "java_maven_dependencies_v1");
    assert_eq!(
        capability_group_members(JAVA_MAVEN_LEAF_GROUP),
        JAVA_MAVEN_CAPABILITIES
    );
    assert_eq!(
        capability_group_members(JAVA_MAVEN_DEPENDENCIES_GROUP),
        &[JAVA_MAVEN_DEPENDENCIES_CAPABILITY]
    );

    // Metadata validity is structural. Dispatch owns lifecycle prerequisites,
    // trust, session ownership and host implementation support.
    for os in ["linux", "windows", "future-os"] {
        let mut leaf = info(&[JAVA_MAVEN_LEAF_GROUP]);
        leaf.os = os.into();
        leaf.validate().unwrap();
        let mut dependencies = info(&[JAVA_MAVEN_DEPENDENCIES_GROUP]);
        dependencies.os = os.into();
        dependencies.validate().unwrap();
        for operation in JAVA_MAVEN_CAPABILITIES {
            assert!(supports_capability(Some(&leaf), operation));
            assert!(!dependencies.supports(operation));
            assert!(!supports_capability(None, operation));
        }
        assert!(!leaf.supports(JAVA_MAVEN_DEPENDENCIES_CAPABILITY));
        assert!(dependencies.supports(JAVA_MAVEN_DEPENDENCIES_CAPABILITY));
        assert!(!supports_capability(
            None,
            JAVA_MAVEN_DEPENDENCIES_CAPABILITY
        ));
        for name in JAVA_LANGUAGE_SESSION_CAPABILITIES
            .iter()
            .chain(JAVA_STARTUP_CAPABILITIES)
            .chain(&[
                "list",
                "read",
                "run",
                "run_start",
                "language_start",
                "language_execute_command",
                "execute_command",
                JAVA_MAVEN_LEAF_GROUP,
                JAVA_MAVEN_DEPENDENCIES_GROUP,
            ])
        {
            assert!(!leaf.supports(name), "leaf unexpectedly claims {name}");
            assert!(
                !dependencies.supports(name),
                "dependencies unexpectedly claims {name}"
            );
        }
    }
}

#[test]
fn well_formed_unknown_groups_and_versions_are_inert() {
    for group in [
        "future.group-v1",
        "java_maven_leaf",
        "java_maven_leaf_v0",
        "java_maven_leaf_v2",
        "java_maven_leaf_v10",
        "java_maven_leaf_v1.extra",
        "java_maven_dependencies_v2",
        "language_maven_model",
        "language_maven_dependencies",
    ] {
        let mut advertised = info(&[group]);
        advertised.capabilities = vec!["read".into(), "future_operation".into()];
        advertised.validate().unwrap();
        assert!(capability_group_members(group).is_empty());
        assert!(advertised.supports("read"));
        assert!(advertised.supports("future_operation"));
        assert!(!advertised.supports(group));
        for name in JAVA_MAVEN_CAPABILITIES
            .iter()
            .chain(&[JAVA_MAVEN_DEPENDENCIES_CAPABILITY])
        {
            assert!(!advertised.supports(name), "unknown group {group}");
        }
        let encoded = serde_json::to_value(&advertised).unwrap();
        assert_eq!(encoded["capability_groups"], json!([group]));
        assert_eq!(
            serde_json::from_value::<AgentInfo>(encoded).unwrap(),
            advertised
        );
    }
}

#[test]
fn direct_and_group_claims_form_an_idempotent_union_without_flattening() {
    let mut advertised = info(&[JAVA_MAVEN_LEAF_GROUP, JAVA_MAVEN_DEPENDENCIES_GROUP]);
    advertised.capabilities = JAVA_MAVEN_CAPABILITIES
        .iter()
        .chain(&[JAVA_MAVEN_DEPENDENCIES_CAPABILITY, "read"])
        .map(|name| (*name).into())
        .collect();
    advertised.validate().unwrap();
    let original = advertised.clone();
    for _ in 0..3 {
        for name in &original.capabilities {
            assert!(advertised.supports(name));
        }
    }
    assert_eq!(advertised, original);
    let encoded = serde_json::to_value(&advertised).unwrap();
    assert_eq!(encoded["capabilities"], json!(original.capabilities));
    assert_eq!(
        encoded["capability_groups"],
        json!(original.capability_groups)
    );

    advertised.capability_groups.clear();
    advertised.validate().unwrap();
    for name in &original.capabilities {
        assert!(advertised.supports(name));
    }
    advertised.capabilities = vec![JAVA_MAVEN_LEAF_GROUP.into()];
    assert!(advertised.supports(JAVA_MAVEN_LEAF_GROUP));
    for name in JAVA_MAVEN_CAPABILITIES {
        assert!(
            !advertised.supports(name),
            "flat identifiers are never expanded"
        );
    }
}

#[test]
fn group_types_null_elements_and_nested_arrays_are_rejected() {
    for invalid in [
        Value::Null,
        json!(false),
        json!(1),
        json!(JAVA_MAVEN_LEAF_GROUP),
        json!({"group": JAVA_MAVEN_LEAF_GROUP}),
        json!([null]),
        json!([1]),
        json!([false]),
        json!([[JAVA_MAVEN_LEAF_GROUP]]),
        json!([JAVA_MAVEN_LEAF_GROUP, {}]),
    ] {
        let mut wire = serde_json::to_value(info(&[])).unwrap();
        wire["capability_groups"] = invalid.clone();
        assert!(
            serde_json::from_value::<AgentInfo>(wire.clone()).is_err(),
            "{invalid}"
        );
        let mut response = serde_json::to_value(hello(info(&[]))).unwrap();
        response["result"]["Ok"]["agent"] = wire;
        assert!(
            serde_json::from_value::<Response>(response).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn group_validation_rejects_malformed_duplicate_and_excess_identifiers() {
    for invalid in [
        "",
        "Java_maven_leaf_v1",
        "java/maven",
        "java maven",
        "java\n",
        "java\0",
        "java\u{7f}",
        "é",
        "java\\maven",
        "java\"maven",
    ] {
        let advertised = info(&[invalid]);
        assert_eq!(
            advertised.validate().unwrap_err().code,
            "invalid_agent_info"
        );
    }
    for duplicate in [
        JAVA_MAVEN_LEAF_GROUP,
        JAVA_MAVEN_DEPENDENCIES_GROUP,
        "future_v1",
    ] {
        let advertised = info(&[duplicate, duplicate]);
        assert_eq!(
            advertised.validate().unwrap_err().code,
            "invalid_agent_info"
        );
    }
    let too_many = info(&[
        JAVA_MAVEN_LEAF_GROUP,
        JAVA_MAVEN_DEPENDENCIES_GROUP,
        "future_v1",
    ]);
    assert_eq!(too_many.validate().unwrap_err().code, "invalid_agent_info");
    let too_long = info(&[&"a".repeat(MAX_CAPABILITY_GROUP_BYTES + 1)]);
    assert_eq!(too_long.validate().unwrap_err().code, "invalid_agent_info");
}

#[test]
fn independent_flat_and_group_limits_bound_identifiers_and_canonical_json() {
    assert_eq!(MAX_AGENT_CAPABILITY_GROUPS, 2);
    assert_eq!(MAX_CAPABILITY_GROUP_BYTES, 64);
    assert_eq!(MAX_CAPABILITY_GROUPS_IDENTIFIER_BYTES, 128);
    assert_eq!(MAX_CAPABILITY_GROUPS_JSON_BYTES, 135);
    for (groups, identifier_bytes, json_bytes) in [
        (Vec::new(), 0, 2),
        (vec!["a".into()], 1, 5),
        (vec!["a".repeat(64)], 64, 68),
        (vec!["a".repeat(64), "b".repeat(64)], 128, 135),
    ] {
        let mut advertised = info(&[]);
        advertised.capability_groups = groups;
        advertised.capabilities = (0..MAX_AGENT_CAPABILITIES)
            .map(|index| format!("flat_{index}"))
            .collect();
        advertised.validate().unwrap();
        assert_eq!(
            advertised
                .capability_groups
                .iter()
                .map(String::len)
                .sum::<usize>(),
            identifier_bytes
        );
        assert_eq!(
            serde_json::to_vec(&advertised.capability_groups)
                .unwrap()
                .len(),
            json_bytes
        );
        advertised.capabilities.push("one_too_many".into());
        assert_eq!(
            advertised.validate().unwrap_err().code,
            "invalid_agent_info"
        );
    }
    let mut maximum = info(&[]);
    maximum.capability_groups = vec!["a".repeat(64), "b".repeat(64)];
    maximum.capability_groups[1].push('b');
    assert!(maximum.validate().is_err());

    // Bounds apply to decoded ASCII identifiers and their canonical array,
    // not arbitrary wire whitespace or escape spellings (frame bounds do).
    let encoded = serde_json::to_string(&info(&[])).unwrap();
    let escaped = format!(
        r#"{{"capability_groups": [ "java_maven_leaf_v\u0031" ],{}"#,
        &encoded[1..]
    );
    let advertised: AgentInfo = serde_json::from_str(&escaped).unwrap();
    advertised.validate().unwrap();
    assert_eq!(advertised.capability_groups, [JAVA_MAVEN_LEAF_GROUP]);
    assert!(advertised.supports("language_maven_model"));
}

#[test]
fn duplicate_group_keys_are_rejected_by_raw_typed_readers() {
    let metadata = serde_json::to_string(&info(&[JAVA_MAVEN_LEAF_GROUP])).unwrap();
    for first in [
        "null",
        "[]",
        r#"["future_v1"]"#,
        r#"["java_maven_leaf_v1"]"#,
    ] {
        let duplicate = format!(r#"{{"capability_groups":{first},{}"#, &metadata[1..]);
        assert!(serde_json::from_str::<AgentInfo>(&duplicate).is_err());
        let raw = format!(
            r#"{{"id":38,"result":{{"Ok":{{"type":"hello","protocol":4,"root":"/fixture","agent":{duplicate}}}}}}}"#
        );
        assert!(serde_json::from_str::<Response>(&raw).is_err());
        let framed = raw + "\n";
        assert!(read_frame::<_, Response>(&mut framed.as_bytes()).is_err());

        // serde_json::Value intentionally keeps the last object value, so a
        // Value-mediated client cannot recover or reject erased duplicate keys.
        let value: Value = serde_json::from_str(&duplicate).unwrap();
        let decoded: AgentInfo = serde_json::from_value(value).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.capability_groups, [JAVA_MAVEN_LEAF_GROUP]);
    }
    let duplicated_id = info(&[JAVA_MAVEN_LEAF_GROUP, JAVA_MAVEN_LEAF_GROUP]);
    let value = serde_json::to_value(duplicated_id).unwrap();
    let decoded: AgentInfo = serde_json::from_value(value).unwrap();
    assert!(
        decoded.validate().is_err(),
        "array duplicates survive Value parsing"
    );
}

#[test]
fn frozen_037_reader_accepts_grouped_hello_and_retains_only_31_flat_claims() {
    let mut advertised = info(&[JAVA_MAVEN_LEAF_GROUP, JAVA_MAVEN_DEPENDENCIES_GROUP]);
    advertised.capabilities = LEGACY_31_CAPABILITIES
        .iter()
        .map(|name| (*name).into())
        .collect();
    advertised.validate().unwrap();
    assert_eq!(advertised.capabilities.len(), 31);
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &hello(advertised.clone())).unwrap();

    let old: frozen_v037::Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
    assert_eq!(old.id, 38);
    let frozen_v037::Payload::Hello {
        protocol,
        root,
        agent,
    } = old.result.unwrap();
    assert_eq!(protocol, 4);
    assert_eq!(root, "/fixture");
    let old = agent.unwrap();
    old.validate().unwrap();
    assert_eq!(old.capabilities, advertised.capabilities);
    for name in LEGACY_31_CAPABILITIES {
        assert!(old.supports(name));
        assert!(advertised.supports(name));
    }
    for name in JAVA_MAVEN_CAPABILITIES
        .iter()
        .chain(&[JAVA_MAVEN_DEPENDENCIES_CAPABILITY])
    {
        assert!(!frozen_v037::supports_capability(Some(&old), name));
        assert!(advertised.supports(name));
    }
    assert!(serde_json::to_value(&old)
        .unwrap()
        .get("capability_groups")
        .is_none());
    let current: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
    let Payload::Hello {
        agent: Some(current),
        ..
    } = current.result.unwrap()
    else {
        panic!("expected current metadata");
    };
    current.validate().unwrap();
    assert_eq!(current, advertised);
}

#[test]
fn known_groups_do_not_consume_the_frozen_flat_capability_budget() {
    let mut advertised = info(&[JAVA_MAVEN_LEAF_GROUP, JAVA_MAVEN_DEPENDENCIES_GROUP]);
    advertised.capabilities = LEGACY_31_CAPABILITIES
        .iter()
        .map(|name| (*name).into())
        .collect();
    advertised.capabilities.push("future_flat".into());
    assert_eq!(advertised.capabilities.len(), 32);
    advertised.validate().unwrap();
    assert!(advertised.supports("language_maven_model"));
    assert!(advertised.supports(JAVA_MAVEN_DEPENDENCIES_CAPABILITY));
    let old: frozen_v037::AgentInfo =
        serde_json::from_value(serde_json::to_value(&advertised).unwrap()).unwrap();
    old.validate().unwrap();
    assert!(old.supports("future_flat"));
    assert!(!old.supports("language_maven_model"));

    advertised.capabilities.push("one_too_many".into());
    assert!(advertised.validate().is_err());
    let old: frozen_v037::AgentInfo =
        serde_json::from_value(serde_json::to_value(advertised).unwrap()).unwrap();
    assert!(old.validate().is_err());
}

#[test]
fn current_reader_preserves_frozen_windows_direct_claims_and_metadata_free_fallback() {
    let direct: Vec<_> = LEGACY_31_CAPABILITIES
        .iter()
        .copied()
        .filter(|name| !matches!(*name, "git_status" | "run" | "language_start"))
        .chain(JAVA_MAVEN_CAPABILITIES.iter().copied())
        .chain([JAVA_MAVEN_DEPENDENCIES_CAPABILITY])
        .collect();
    let old: frozen_v037::AgentInfo = serde_json::from_value(json!({
        "schema": 1, "version": "0.37.0", "os": "windows", "arch": "x86_64",
        "capabilities": direct,
    }))
    .unwrap();
    old.validate().unwrap();
    let old_response = frozen_v037::Response {
        id: 37,
        result: Ok(frozen_v037::Payload::Hello {
            protocol: 4,
            root: "C:/fixture".into(),
            agent: Some(old),
        }),
    };
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &old_response).unwrap();
    let current: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
    let Payload::Hello {
        agent: Some(current),
        ..
    } = current.result.unwrap()
    else {
        panic!("expected direct metadata");
    };
    current.validate().unwrap();
    assert!(current.capability_groups.is_empty());
    for name in direct {
        assert!(current.supports(name));
    }
    for name in ["list", "read", "write", "search"] {
        assert!(frozen_v037::supports_capability(None, name));
        assert!(supports_capability(None, name));
    }
    for name in JAVA_MAVEN_CAPABILITIES
        .iter()
        .chain(&[JAVA_MAVEN_DEPENDENCIES_CAPABILITY])
    {
        assert!(!frozen_v037::supports_capability(None, name));
        assert!(!supports_capability(None, name));
    }
}

// Frozen source: Cedar 0.37 schema copied from local source commit
// ec11efaa7540af541be559e9f5a62af5026a1f36, crates/protocol/src/lib.rs.
// The same protocol source is published at checkpoint
// 8b8617e349f9bb224a7178770d339743852bad46, crates/protocol/src/lib.rs.
// Constants, AgentInfo, validate, valid_identifier, supports, supports_capability,
// Response and RemoteError are copied verbatim (module indentation only).
// Payload retains the verbatim Hello variant only, since no other payload is
// exercised. Do not replace these with current protocol aliases or validators.
mod frozen_v037 {
    use serde::{Deserialize, Serialize};

    pub const AGENT_INFO_SCHEMA: u32 = 1;
    pub const MAX_AGENT_VERSION_BYTES: usize = 64;
    pub const MAX_AGENT_PLATFORM_BYTES: usize = 32;
    pub const MAX_AGENT_CAPABILITIES: usize = 32;
    pub const MAX_CAPABILITY_BYTES: usize = 64;

    /// Unverified implementation information, never execution permission or identity.
    /// Validate received information before retaining it as a connection snapshot.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct AgentInfo {
        pub schema: u32,
        pub version: String,
        pub os: String,
        pub arch: String,
        pub capabilities: Vec<String>,
    }

    impl AgentInfo {
        pub fn validate(&self) -> Result<(), RemoteError> {
            let invalid = |message| RemoteError::new("invalid_agent_info", message);
            if self.schema != AGENT_INFO_SCHEMA {
                return Err(invalid("Unsupported agent metadata schema"));
            }
            if self.version.is_empty()
                || self.version.len() > MAX_AGENT_VERSION_BYTES
                || !self
                    .version
                    .bytes()
                    .all(|b| b.is_ascii() && !b.is_ascii_control())
            {
                return Err(invalid("Agent version must be 1..64 printable ASCII bytes"));
            }
            for value in [&self.os, &self.arch] {
                if !valid_identifier(value, MAX_AGENT_PLATFORM_BYTES) {
                    return Err(invalid(
                        "Agent platform identifiers must be 1..32 lowercase ASCII identifier bytes",
                    ));
                }
            }
            if self.capabilities.len() > MAX_AGENT_CAPABILITIES {
                return Err(invalid("Agent metadata exceeds 32 capabilities"));
            }
            for (index, capability) in self.capabilities.iter().enumerate() {
                if !valid_identifier(capability, MAX_CAPABILITY_BYTES) {
                    return Err(invalid(
                        "Agent capabilities must be 1..64 lowercase ASCII identifier bytes",
                    ));
                }
                if self.capabilities[..index].contains(capability) {
                    return Err(invalid("Agent capabilities must be unique"));
                }
            }
            Ok(())
        }

        /// Returns only a support claim. Callers must separately enforce trust,
        /// operation-family prerequisites, and current connection/session state.
        pub fn supports(&self, name: &str) -> bool {
            self.capabilities
                .iter()
                .any(|capability| capability == name)
        }
    }

    fn valid_identifier(value: &str, max_bytes: usize) -> bool {
        !value.is_empty()
            && value.len() <= max_bytes
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    }

    /// Legacy protocol-4 peers retain basic editing; execution support requires
    /// explicit metadata. An empty declared list never falls back to legacy support.
    pub fn supports_capability(agent: Option<&AgentInfo>, name: &str) -> bool {
        match agent {
            Some(agent) => agent.supports(name),
            None => matches!(name, "list" | "read" | "write" | "search"),
        }
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Response {
        pub id: u64,
        pub result: Result<Payload, RemoteError>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Payload {
        Hello {
            protocol: u32,
            root: String,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            agent: Option<AgentInfo>,
        },
    }
    #[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
    #[error("{code}: {message}")]
    pub struct RemoteError {
        pub code: String,
        pub message: String,
    }
    impl RemoteError {
        pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
            Self {
                code: code.into(),
                message: message.into(),
            }
        }
    }
}
