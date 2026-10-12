use cedar_protocol::{
    read_frame, write_frame, AgentInfo, FileDiscoveryDescriptor,
    FileDiscoveryIncompleteReason as Reason, FileDiscoveryResult, Operation, Payload, Response,
    FILE_DISCOVERY_VERSION, MAX_FILE_DISCOVERY_DESCRIPTOR_BYTES, MAX_FILE_DISCOVERY_RESULT_BYTES,
};
use serde_json::json;

// Exact accepted source snapshots, including original enum/Serde attributes,
// validators, reader and existing unit tests. Never substitute current aliases.
// Source: commit 1e2a26d6dd38c17ed3723ced54797fcb43255eb6.
// lib.rs blob: 30cb280f4f4aaadfea861f76037b5d90fb45a7f5.
// maven_dependencies.rs blob: 8707970b7d2ab118074b34e5090e654cc8e3212a.
#[allow(dead_code)]
#[path = "fixtures/accepted_0593/lib.rs"]
mod accepted_0593;
// The accepted maven_dependencies module uses this original crate-root name.
use accepted_0593::RemoteError;

fn agent() -> AgentInfo {
    AgentInfo {
        schema: 1,
        version: "0.59.3".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        capabilities: vec!["list".into(), "read".into()],
        capability_groups: vec![],
    }
}

fn hello(descriptor: Option<FileDiscoveryDescriptor>) -> Response {
    Response {
        id: 59,
        result: Ok(Payload::Hello {
            protocol: 4,
            root: "/fixture".into(),
            agent: Some(agent()),
            file_discovery: descriptor,
            scoped_search: None,
        }),
    }
}

fn raw_hello(fields: &str) -> String {
    format!("{{\"id\":59,\"result\":{{\"Ok\":{{\"type\":\"hello\",\"protocol\":4,\"root\":\"/fixture\",{fields}}}}}}}\n")
}

#[test]
fn descriptor_is_independent_positive_and_has_exact_compact_cardinality_and_size() {
    assert_eq!(FILE_DISCOVERY_VERSION, 1);
    assert_eq!(MAX_FILE_DISCOVERY_DESCRIPTOR_BYTES, 22);
    for version in [1, 2, u32::MAX] {
        let descriptor = FileDiscoveryDescriptor { version };
        descriptor.validate().unwrap();
        let bytes = serde_json::to_vec(&descriptor).unwrap();
        assert_eq!(bytes.len(), if version == u32::MAX { 22 } else { 13 });
        assert!(bytes.len() <= MAX_FILE_DISCOVERY_DESCRIPTOR_BYTES);
        assert_eq!(
            serde_json::from_slice::<FileDiscoveryDescriptor>(&bytes).unwrap(),
            descriptor
        );
        assert_eq!(descriptor.supported(), version == 1);
        assert_eq!(
            serde_json::to_value(descriptor)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            1
        );
    }
    assert!(FileDiscoveryDescriptor { version: 0 }.validate().is_err());
    assert_eq!(cedar_protocol::PROTOCOL_VERSION, 4);
    assert_eq!(cedar_protocol::AGENT_INFO_SCHEMA, 1);
    assert_eq!(cedar_protocol::MAX_AGENT_CAPABILITIES, 32);
    assert_eq!(cedar_protocol::MAX_AGENT_CAPABILITY_GROUPS, 2);
}

#[test]
fn raw_descriptor_negatives_reject_outer_and_inner_duplicates_even_null_and_escaped_names() {
    for value in [
        "{}",
        "[]",
        "true",
        "1",
        "\"v1\"",
        "{\"version\":0}",
        "{\"version\":-1}",
        "{\"version\":1.0}",
        "{\"version\":4294967296}",
        "{\"version\":null}",
        "{\"version\":\"1\"}",
        "{\"version\":[]}",
        "{\"version\":{}}",
        "{\"version\":true}",
        "{\"version\":1,\"extra\":null}",
        "{\"version\":1,\"version\":1}",
        "{\"version\":null,\"version\":1}",
        "{\"version\":1,\"versi\\u006fn\":2}",
    ] {
        let raw = raw_hello(&format!("\"file_discovery\":{value}"));
        assert!(
            read_frame::<_, Response>(&mut raw.as_bytes()).is_err(),
            "accepted {value}"
        );
    }
    for first in ["null", "{\"version\":1}", "{\"version\":2}"] {
        for second in ["null", "{\"version\":1}"] {
            for key in ["file_discovery", "file_discov\\u0065ry"] {
                let raw = raw_hello(&format!("\"file_discovery\":{first},\"{key}\":{second}"));
                assert!(
                    read_frame::<_, Response>(&mut raw.as_bytes()).is_err(),
                    "accepted {raw}"
                );
            }
        }
    }
    // Whitespace and escape spellings are frame-bounded, not a 22-byte raw cap.
    let raw = raw_hello("\"file_discovery\": { \"versi\\u006fn\" : 1 }");
    assert!(read_frame::<_, Response>(&mut raw.as_bytes())
        .unwrap()
        .is_some());
}

#[test]
fn accepted_0593_reader_ignores_extension_and_preserves_all_old_validation() {
    let mut response = hello(Some(FileDiscoveryDescriptor::V1));
    let Payload::Hello {
        agent: Some(info), ..
    } = response.result.as_mut().unwrap()
    else {
        panic!();
    };
    info.capabilities = (0..32).map(|i| format!("flat_{i}")).collect();
    info.capabilities[0] = "list".into();
    info.capabilities[1] = "read".into();
    info.capability_groups = vec![
        cedar_protocol::JAVA_MAVEN_LEAF_GROUP.into(),
        cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP.into(),
    ];
    let expected = info.clone();
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &response).unwrap();
    let old: accepted_0593::Response = accepted_0593::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let accepted_0593::Payload::Hello {
        protocol,
        root,
        agent: Some(old),
    } = old.result.unwrap()
    else {
        panic!();
    };
    old.validate().unwrap();
    assert_eq!(protocol, 4);
    assert_eq!(root, "/fixture");
    assert_eq!(old.capabilities, expected.capabilities);
    assert_eq!(old.capability_groups, expected.capability_groups);
    assert!(old.supports("language_maven_model"));
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let mut invalid = old.clone();
    invalid.capabilities.push("overflow".into());
    assert!(invalid.validate().is_err());
    let mut invalid = old.clone();
    invalid.capability_groups.push("overflow".into());
    assert!(invalid.validate().is_err());
    let raw = raw_hello("\"agent\":null,\"agent\":null,\"file_discovery\":{\"version\":1}");
    assert!(accepted_0593::read_frame::<_, accepted_0593::Response>(&mut raw.as_bytes()).is_err());
    // An accepted peer still cannot decode the new operation. The client gate,
    // rather than speculative requests, preserves that boundary.
    let operation = Operation::FindFiles {
        query: "file".into(),
        scope: "".into(),
        limit: 100,
    };
    assert!(serde_json::from_value::<accepted_0593::Operation>(
        serde_json::to_value(operation).unwrap()
    )
    .is_err());
    let old_hello = accepted_0593::Response {
        id: 60,
        result: Ok(accepted_0593::Payload::Hello {
            protocol: 4,
            root: "/old".into(),
            agent: Some(old),
        }),
    };
    bytes.clear();
    accepted_0593::write_frame(&mut bytes, &old_hello).unwrap();
    let current: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
    assert!(matches!(
        current.result,
        Ok(Payload::Hello {
            file_discovery: None,
            scoped_search: None,
            ..
        })
    ));
}

#[test]
fn absent_null_and_future_descriptors_preserve_unavailable_or_unsupported_state() {
    for fields in ["\"agent\":null", "\"file_discovery\":null"] {
        let raw = raw_hello(fields);
        let current: Response = read_frame(&mut raw.as_bytes()).unwrap().unwrap();
        assert!(matches!(
            current.result,
            Ok(Payload::Hello {
                file_discovery: None,
                scoped_search: None,
                ..
            })
        ));
    }
    let future = hello(Some(FileDiscoveryDescriptor { version: u32::MAX }));
    let current: Response = serde_json::from_value(serde_json::to_value(future).unwrap()).unwrap();
    assert!(
        matches!(current.result, Ok(Payload::Hello { file_discovery: Some(descriptor), .. }) if !descriptor.supported())
    );
    let operation: Operation =
        serde_json::from_value(json!({"type":"find_files","query":"file","scope":""})).unwrap();
    assert!(matches!(operation, Operation::FindFiles { limit: 100, .. }));
    assert_eq!(operation.capability_name(), None);
}

fn result() -> FileDiscoveryResult {
    FileDiscoveryResult {
        query: "file".into(),
        scope: "src".into(),
        paths: vec!["src/file".into(), "src/file.txt".into()],
        incomplete: false,
        reasons: vec![],
        examined_entries: 3,
    }
}

#[test]
fn results_validate_scope_match_rank_uniqueness_completeness_and_each_bound() {
    result().validate_for("file", "src", 100).unwrap();
    type ResultMutation = Box<dyn Fn(&mut FileDiscoveryResult)>;
    let mutations: Vec<ResultMutation> = vec![
        Box::new(|r| r.query = "other".into()),
        Box::new(|r| r.scope = "other".into()),
        Box::new(|r| r.paths[0] = "src-other/file".into()),
        Box::new(|r| r.paths[0] = "src/../file".into()),
        Box::new(|r| r.paths[0] = "src//file".into()),
        Box::new(|r| r.paths[0] = "src/unrelated".into()),
        Box::new(|r| r.paths[0] = "src/target/file".into()),
        Box::new(|r| r.paths.swap(0, 1)),
        Box::new(|r| r.paths[1] = r.paths[0].clone()),
        Box::new(|r| r.incomplete = true),
        Box::new(|r| r.reasons.push(Reason::TimeLimit)),
        Box::new(|r| r.examined_entries = 20_001),
        Box::new(|r| r.examined_entries = 1),
        Box::new(|r| {
            r.incomplete = true;
            r.reasons = vec![Reason::TimeLimit; 2];
        }),
        Box::new(|r| r.paths[0] = format!("src/file{}", "a".repeat(4096))),
    ];
    for mutate in mutations {
        let mut invalid = result();
        mutate(&mut invalid);
        assert!(
            invalid.validate_for("file", "src", 100).is_err(),
            "{invalid:?}"
        );
    }
    assert!(result().validate_for("file", "src", 1).is_err());
    let mut incomplete = result();
    incomplete.paths.clear();
    incomplete.incomplete = true;
    incomplete.reasons.push(Reason::PermissionDenied);
    incomplete.validate_for("file", "src", 100).unwrap();
}

#[test]
fn result_json_bound_accounts_for_escaping_not_only_decoded_path_bytes() {
    let mut result = result();
    result.paths = (0..200)
        .map(|i| format!("src/file{i:03}{}", "\"".repeat(2000)))
        .collect();
    result.examined_entries = 200;
    assert!(result.paths.iter().map(String::len).sum::<usize>() < MAX_FILE_DISCOVERY_RESULT_BYTES);
    assert!(serde_json::to_vec(&result).unwrap().len() > MAX_FILE_DISCOVERY_RESULT_BYTES);
    assert!(result.validate_for("file", "src", 200).is_err());
}

#[test]
fn lowercase_policy_does_not_normalize_or_full_casefold_original_paths() {
    assert_eq!(
        cedar_protocol::file_discovery_rank("项目/Ä.txt", "ä"),
        Some(1)
    );
    assert_eq!(
        cedar_protocol::file_discovery_rank("项目/Straße", "STRASSE"),
        None
    );
    assert_eq!(
        cedar_protocol::file_discovery_rank("项目/é", "e\u{301}"),
        None
    );
}

#[test]
fn scoped_depth_64_is_admitted_and_65_is_rejected() {
    let mut result = result();
    result.paths = vec![format!("src/{}file", "d/".repeat(64))];
    result.validate_for("file", "src", 100).unwrap();
    result.paths = vec![format!("src/{}file", "d/".repeat(65))];
    assert!(result.validate_for("file", "src", 100).is_err());
}
