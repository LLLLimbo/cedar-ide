// Execute the actual unchanged pre-extension reader, writer and validators.
#[allow(dead_code)]
#[path = "fixtures/accepted_079/lib.rs"]
mod accepted_079;
// Original child modules' crate-relative imports resolve to their own types.
use accepted_079::{
    valid_file_discovery_path, RemoteError, SearchMatch, FILE_DISCOVERY_EXCLUDED_DIRECTORIES,
    MAX_FILE_BYTES,
};
use cedar_protocol as current;

fn hello() -> current::Response {
    current::Response {
        id: 79,
        result: Ok(current::Payload::Hello {
            protocol: current::PROTOCOL_VERSION,
            root: "/fixture".into(),
            agent: Some(current::AgentInfo {
                schema: current::AGENT_INFO_SCHEMA,
                version: "0.79.0".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                capabilities: (0..32).map(|n| format!("flat_{n}")).collect(),
                capability_groups: vec![
                    current::JAVA_MAVEN_LEAF_GROUP.into(),
                    current::JAVA_MAVEN_DEPENDENCIES_GROUP.into(),
                ],
            }),
            file_discovery: Some(current::FileDiscoveryDescriptor::V1),
            scoped_search: Some(current::ScopedSearchDescriptor::V1),
            case_search: Some(current::CaseSearchDescriptor::V1),
        }),
    }
}

#[test]
fn actual_079_reader_accepts_current_hello_and_preserves_both_existing_descriptors() {
    let mut bytes = Vec::new();
    current::write_frame(&mut bytes, &hello()).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("\"case_search\""));
    let old: accepted_079::Response = accepted_079::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let accepted_079::Payload::Hello {
        protocol,
        root,
        agent: Some(mut agent),
        file_discovery: Some(discovery),
        scoped_search: Some(scoped),
    } = old.result.unwrap()
    else {
        panic!()
    };
    assert_eq!(protocol, 4);
    assert_eq!(root, "/fixture");
    agent.validate().unwrap();
    discovery.validate().unwrap();
    scoped.validate().unwrap();
    assert!(discovery.supported());
    assert!(scoped.supported());
    assert_eq!(agent.capabilities.len(), 32);
    assert_eq!(agent.capability_groups.len(), 2);
    agent.capabilities.push("overflow".into());
    assert!(agent.validate().is_err());
    agent.capabilities.pop();
    agent.capability_groups.push("overflow".into());
    assert!(agent.validate().is_err());
}

#[test]
fn actual_079_scoped_request_result_and_hello_round_trip_remain_usable() {
    let request = accepted_079::ScopedSearchRequest {
        query: "needle".into(),
        scope: "src".into(),
        masks: vec!["*.rs".into()],
        limit: 1234,
    };
    assert_eq!(request.validate().unwrap(), 1000);
    let old = accepted_079::Request {
        id: 80,
        op: accepted_079::Operation::SearchIn {
            request: request.clone(),
        },
    };
    let mut bytes = Vec::new();
    accepted_079::write_frame(&mut bytes, &old).unwrap();
    let new: current::Request = current::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let current::Operation::SearchIn {
        request: new_request,
    } = new.op
    else {
        panic!()
    };
    new_request.validate().unwrap();
    assert_eq!(new_request.limit, 1234);
    bytes.clear();
    current::write_frame(
        &mut bytes,
        &current::Request {
            id: 80,
            op: current::Operation::SearchIn {
                request: new_request.clone(),
            },
        },
    )
    .unwrap();
    let old_again: accepted_079::Request =
        accepted_079::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let accepted_079::Operation::SearchIn {
        request: old_request,
    } = old_again.op
    else {
        panic!()
    };
    assert_eq!(old_request, request);
    let old_result = accepted_079::ScopedSearchResult {
        request: request.clone(),
        matches: vec![SearchMatch {
            path: "src/a.rs".into(),
            line: 1,
            text: "needle".into(),
        }],
        incomplete: false,
        reasons: vec![],
        examined_entries: 1,
        read_bytes: 7,
    };
    old_result.validate_for(&request).unwrap();
    bytes.clear();
    accepted_079::write_frame(
        &mut bytes,
        &accepted_079::Response {
            id: 80,
            result: Ok(accepted_079::Payload::ScopedMatches { result: old_result }),
        },
    )
    .unwrap();
    let new: current::Response = current::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let current::Payload::ScopedMatches { result: new_result } = new.result.unwrap() else {
        panic!()
    };
    new_result.validate_for(&new_request).unwrap();
    bytes.clear();
    current::write_frame(
        &mut bytes,
        &current::Response {
            id: 80,
            result: Ok(current::Payload::ScopedMatches { result: new_result }),
        },
    )
    .unwrap();
    let old: accepted_079::Response = accepted_079::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let accepted_079::Payload::ScopedMatches { result: old_result } = old.result.unwrap() else {
        panic!()
    };
    old_result.validate_for(&request).unwrap();
    assert_eq!(old_result.matches[0].text, "needle");
    let old_hello = accepted_079::Response {
        id: 79,
        result: Ok(accepted_079::Payload::Hello {
            protocol: 4,
            root: "/old".into(),
            agent: None,
            file_discovery: Some(accepted_079::FileDiscoveryDescriptor::V1),
            scoped_search: Some(accepted_079::ScopedSearchDescriptor::V1),
        }),
    };
    bytes.clear();
    accepted_079::write_frame(&mut bytes, &old_hello).unwrap();
    let new: current::Response = current::read_frame(&mut &bytes[..]).unwrap().unwrap();
    assert!(
        matches!(new.result,Ok(current::Payload::Hello{case_search:None,scoped_search:Some(d),..}) if d.supported())
    );
}

#[test]
fn actual_079_reader_rejects_case_operation_when_deliberately_supplied() {
    let request = current::Request {
        id: 81,
        op: current::Operation::SearchCase {
            request: current::CaseSearchRequest {
                query: "needle".into(),
                scope: String::new(),
                masks: vec![],
                limit: 100,
                case_sensitive: false,
            },
        },
    };
    let mut bytes = Vec::new();
    current::write_frame(&mut bytes, &request).unwrap();
    assert!(accepted_079::read_frame::<_, accepted_079::Request>(&mut &bytes[..]).is_err());
}

#[test]
fn fixture_provenance_records_all_four_source_lengths_and_hash_identities() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/accepted_079/PROVENANCE.json")).unwrap();
    assert_eq!(
        manifest["commit"],
        "705457c6ac67cc6e7be62490e17b5d2277af0d08"
    );
    assert_eq!(manifest["tree"], "569ae835d76e908e1d6876d58366a3f1046dde2a");
    let sources = [
        (
            "lib.rs",
            include_bytes!("fixtures/accepted_079/lib.rs").as_slice(),
        ),
        (
            "file_discovery.rs",
            include_bytes!("fixtures/accepted_079/file_discovery.rs").as_slice(),
        ),
        (
            "maven_dependencies.rs",
            include_bytes!("fixtures/accepted_079/maven_dependencies.rs").as_slice(),
        ),
        (
            "scoped_search.rs",
            include_bytes!("fixtures/accepted_079/scoped_search.rs").as_slice(),
        ),
    ];
    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), 4);
    for (name, bytes) in sources {
        let entry = files.iter().find(|v| v["path"] == name).unwrap();
        assert_eq!(entry["bytes"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(entry["git_blob"].as_str().unwrap().len(), 40);
    }
}
