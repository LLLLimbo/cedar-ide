use cedar_protocol::{
    read_frame, supports_capability, write_frame, AgentInfo, Operation, Payload, RemoteError,
    Response, AGENT_INFO_SCHEMA, JAVA_LANGUAGE_SESSION_CAPABILITIES, JAVA_STARTUP_CAPABILITIES,
    LANGUAGE_SESSION_CAPABILITIES, MAX_AGENT_CAPABILITIES, MAX_AGENT_PLATFORM_BYTES,
    MAX_AGENT_VERSION_BYTES, MAX_CAPABILITY_BYTES, PROTOCOL_VERSION, RUN_TASK_CAPABILITIES,
};
use serde::Deserialize;
use serde_json::json;

fn agent() -> AgentInfo {
    AgentInfo {
        schema: AGENT_INFO_SCHEMA,
        version: "0.6.0-test".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        capabilities: vec![
            "list".into(),
            "read".into(),
            "write".into(),
            "search".into(),
        ],
    }
}

fn hello(agent: Option<AgentInfo>) -> Response {
    Response {
        id: 7,
        result: Ok(Payload::Hello {
            protocol: PROTOCOL_VERSION,
            root: "/fixture".into(),
            agent,
        }),
    }
}

// Frozen protocol-4 response shape: no dependency on the new Hello fields.
#[derive(Debug, Deserialize)]
struct LegacyResponse {
    id: u64,
    result: Result<LegacyPayload, RemoteError>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LegacyPayload {
    Hello { protocol: u32, root: String },
}

#[test]
fn old_and_new_hello_readers_are_wire_compatible() {
    for metadata in [None, Some(agent())] {
        let response = hello(metadata.clone());
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &response).unwrap();
        let legacy: LegacyResponse = read_frame(&mut &bytes[..]).unwrap().unwrap();
        assert_eq!(legacy.id, 7);
        let LegacyPayload::Hello { protocol, root } = legacy.result.unwrap();
        assert_eq!(protocol, 4);
        assert_eq!(root, "/fixture");
        let current: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
        let Payload::Hello { agent, .. } = current.result.unwrap() else {
            panic!("expected hello");
        };
        assert_eq!(agent, metadata);
    }
    let legacy = json!({"id":7,"result":{"Ok":{"type":"hello","protocol":4,"root":"/fixture"}}});
    assert_eq!(serde_json::to_value(hello(None)).unwrap(), legacy);
    assert_eq!(PROTOCOL_VERSION, 4);
}

#[test]
fn absent_and_null_metadata_are_legacy_but_empty_capabilities_are_explicit() {
    let mut wire = serde_json::to_value(hello(None)).unwrap();
    for value in [None, Some(serde_json::Value::Null)] {
        if let Some(value) = value {
            wire["result"]["Ok"]["agent"] = value;
        }
        let decoded: Response = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            decoded.result,
            Ok(Payload::Hello { agent: None, .. })
        ));
    }
    let mut explicit = agent();
    explicit.capabilities.clear();
    explicit.validate().unwrap();
    for name in ["list", "read", "write", "search"] {
        assert!(supports_capability(None, name));
        assert!(!supports_capability(Some(&explicit), name));
    }
    for name in [
        "git_status",
        "run",
        "run_start",
        "language_start",
        "language_start_java",
        "future",
        "hello",
    ] {
        assert!(!supports_capability(None, name));
    }
}

#[test]
fn unknown_fields_names_and_platforms_are_inert_compatible_claims() {
    let mut wire = serde_json::to_value(hello(Some(agent()))).unwrap();
    wire["result"]["Ok"]["future_field"] = json!({"arbitrary":"ignored"});
    let info = &mut wire["result"]["Ok"]["agent"];
    info["future_field"] = json!([1, 2, 3]);
    info["allow_run"] = json!(true);
    info["os"] = json!("future-os");
    info["arch"] = json!("future_arch");
    info["capabilities"] = json!(["read", "future.operation.v1"]);
    let decoded: Response = serde_json::from_value(wire).unwrap();
    let Payload::Hello {
        agent: Some(agent), ..
    } = decoded.result.unwrap()
    else {
        panic!("expected agent metadata");
    };
    agent.validate().unwrap();
    assert!(agent.supports("future.operation.v1"));
    assert!(!agent.supports("run_start"));
    assert!(!agent.supports("Read"));
    let serialized = serde_json::to_value(agent).unwrap();
    assert!(serialized.get("allow_run").is_none());
    assert!(serialized.get("future_field").is_none());
}

#[test]
fn recognized_metadata_fields_are_required_and_typed() {
    let source = serde_json::to_value(agent()).unwrap();
    for field in ["schema", "version", "os", "arch", "capabilities"] {
        let mut missing = source.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<AgentInfo>(missing).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("schema", json!("1")),
        ("schema", json!(-1)),
        ("schema", json!(1.5)),
        ("version", json!(12)),
        ("version", json!(null)),
        ("os", json!(false)),
        ("arch", json!([])),
        ("capabilities", json!("read")),
        ("capabilities", json!(null)),
        ("capabilities", json!(["read", 1])),
    ] {
        let mut malformed = source.clone();
        malformed[field] = value;
        assert!(
            serde_json::from_value::<AgentInfo>(malformed).is_err(),
            "{field}"
        );
    }
    for malformed in [json!({}), json!(false), json!([]), json!("agent")] {
        let mut wire = serde_json::to_value(hello(None)).unwrap();
        wire["result"]["Ok"]["agent"] = malformed;
        assert!(serde_json::from_value::<Response>(wire).is_err());
    }
}

#[test]
fn raw_duplicate_metadata_keys_are_rejected_including_a_null_first_agent() {
    let metadata = serde_json::to_string(&agent()).unwrap();
    let values = serde_json::to_value(agent()).unwrap();
    // Keep these as raw JSON. Parsing through Value first would erase duplicate
    // keys and fail to exercise the typed reader at the real protocol boundary.
    for field in ["schema", "version", "os", "arch", "capabilities"] {
        let duplicate = format!(r#"{{"{field}":{},{}"#, values[field], &metadata[1..]);
        assert!(
            serde_json::from_str::<AgentInfo>(&duplicate).is_err(),
            "{field}"
        );
    }
    for first in ["null", metadata.as_str()] {
        let duplicate = format!(
            r#"{{"id":1,"result":{{"Ok":{{"type":"hello","protocol":4,"root":"/fixture","agent":{first},"agent":{metadata}}}}}}}"#
        );
        assert!(serde_json::from_str::<Response>(&duplicate).is_err());
    }
}

#[test]
fn metadata_validation_rejects_schema_duplicates_and_invalid_identifiers() {
    let mut info = agent();
    for schema in [0, 2, u32::MAX] {
        info.schema = schema;
        assert_eq!(info.validate().unwrap_err().code, "invalid_agent_info");
    }
    info = agent();
    info.capabilities.push("read".into());
    assert!(info.validate().is_err());
    for invalid in [
        "",
        "Read",
        "run/start",
        "run start",
        "read\n",
        "read\0",
        "é",
        "read\u{7f}",
    ] {
        let mut info = agent();
        info.capabilities = vec![invalid.into()];
        assert!(info.validate().is_err(), "capability {invalid:?}");
        info = agent();
        info.os = invalid.into();
        assert!(info.validate().is_err(), "os {invalid:?}");
        info = agent();
        info.arch = invalid.into();
        assert!(info.validate().is_err(), "arch {invalid:?}");
    }
    for invalid in ["", "v1\n", "v1\t", "v1\0", "é", "v1\u{7f}"] {
        let mut info = agent();
        info.version = invalid.into();
        assert!(info.validate().is_err(), "version {invalid:?}");
    }
}

#[test]
fn validation_enforces_inclusive_byte_and_entry_bounds() {
    let mut info = agent();
    info.version = "~".repeat(MAX_AGENT_VERSION_BYTES);
    info.os = "o".repeat(MAX_AGENT_PLATFORM_BYTES);
    info.arch = "a".repeat(MAX_AGENT_PLATFORM_BYTES);
    info.capabilities = (0..MAX_AGENT_CAPABILITIES)
        .map(|i| format!("{i:02}{}", "c".repeat(MAX_CAPABILITY_BYTES - 2)))
        .collect();
    info.validate().unwrap();
    for field in ["version", "os", "arch", "capability", "count"] {
        let mut too_large = info.clone();
        match field {
            "version" => too_large.version.push('x'),
            "os" => too_large.os.push('x'),
            "arch" => too_large.arch.push('x'),
            "capability" => too_large.capabilities[0].push('x'),
            "count" => too_large.capabilities.push("extra".into()),
            _ => unreachable!(),
        }
        assert!(too_large.validate().is_err(), "{field}");
    }
}

#[test]
fn capability_names_match_each_operation_or_its_explicit_bridge_name() {
    let operations = [
        json!({"type":"hello"}),
        json!({"type":"list","path":""}),
        json!({"type":"read","path":"a"}),
        json!({"type":"write","path":"a","text":"","expected_revision":null}),
        json!({"type":"search","query":"x","limit":1}),
        json!({"type":"git_status"}),
        json!({"type":"language_start","program":"server","args":[]}),
        json!({"type":"language_start_java","java_executable":"C:\\jdk\\bin\\java.exe","distribution":"C:\\JDT 雪","data_directory":"C:\\data 雪"}),
        json!({"type":"language_start_java_begin","java_executable":"java.exe","distribution":"jdt","data_directory":"data"}),
        json!({"type":"language_start_java_poll","startup_id":1}),
        json!({"type":"language_start_java_cancel","startup_id":1}),
        json!({"type":"language_open","path":"a","language_id":"rust","version":1,"text":""}),
        json!({"type":"language_change","path":"a","version":1,"text":""}),
        json!({"type":"language_close","path":"a"}),
        json!({"type":"language_query","path":"a","line":0,"character":0,"kind":"hover"}),
        json!({"type":"language_format","path":"a","version":1,"tab_size":4,"insert_spaces":true}),
        json!({"type":"language_refresh_java_diagnostics","path":"a.java","version":1}),
        json!({"type":"language_organize_java_imports","path":"a.java","version":1}),
        json!({"type":"language_references","path":"a","line":0,"character":0,"include_declaration":true}),
        json!({"type":"language_document_symbols","path":"a"}),
        json!({"type":"language_resolve_uri","uri":"file:///a"}),
        json!({"type":"language_resolve_completion","item":{}}),
        json!({"type":"language_events"}),
        json!({"type":"language_stop"}),
        json!({"type":"run_start","program":"tool","args":[],"timeout_secs":1}),
        json!({"type":"run_poll","task_id":1}),
        json!({"type":"run_cancel","task_id":1}),
        json!({"type":"run","program":"tool","args":[],"timeout_secs":1}),
    ];
    let mut names = Vec::new();
    for wire in operations {
        let operation: Operation = serde_json::from_value(wire.clone()).unwrap();
        if wire["type"] == "hello" {
            assert_eq!(operation.capability_name(), None);
        } else if wire["type"] == "language_refresh_java_diagnostics" {
            assert_eq!(
                operation.capability_name(),
                Some("java_diagnostics_refresh")
            );
            names.push(operation.capability_name().unwrap());
        } else {
            assert_eq!(operation.capability_name(), wire["type"].as_str());
            names.push(operation.capability_name().unwrap());
        }
    }
    for name in RUN_TASK_CAPABILITIES
        .iter()
        .chain(LANGUAGE_SESSION_CAPABILITIES)
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES)
        .chain(JAVA_STARTUP_CAPABILITIES)
    {
        assert!(names.contains(name), "unknown session prerequisite {name}");
    }
    assert_eq!(
        RUN_TASK_CAPABILITIES,
        &["run_start", "run_poll", "run_cancel"]
    );
    assert_eq!(
        LANGUAGE_SESSION_CAPABILITIES,
        &[
            "language_start",
            "language_open",
            "language_change",
            "language_close",
            "language_events",
            "language_stop"
        ]
    );
    assert_eq!(JAVA_LANGUAGE_SESSION_CAPABILITIES[0], "language_start_java");
    assert_eq!(
        &JAVA_LANGUAGE_SESSION_CAPABILITIES[1..],
        &LANGUAGE_SESSION_CAPABILITIES[1..]
    );
    for optional in [
        "java_diagnostics_refresh",
        "language_organize_java_imports",
        "language_query",
        "language_resolve_uri",
        "language_format",
        "language_references",
        "language_document_symbols",
        "language_resolve_completion",
    ] {
        assert!(!LANGUAGE_SESSION_CAPABILITIES.contains(&optional));
        assert!(!JAVA_LANGUAGE_SESSION_CAPABILITIES.contains(&optional));
    }
}

#[test]
fn java_start_is_an_additive_protocol_four_operation_with_literal_paths() {
    let wire = json!({
        "type": "language_start_java",
        "java_executable": "C:\\Program Files\\jdk\\bin\\java.exe",
        "distribution": "C:\\JDT distribution 雪",
        "data_directory": "C:\\workspace data 雪"
    });
    let operation: Operation = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(&operation, Operation::LanguageStartJava { .. }));
    assert_eq!(serde_json::to_value(operation).unwrap(), wire);
    assert_eq!(PROTOCOL_VERSION, 4);
    for field in ["java_executable", "distribution", "data_directory"] {
        let mut missing = wire.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Operation>(missing).is_err(),
            "{field}"
        );
        let mut malformed = wire.clone();
        malformed[field] = json!(["not", "a", "path"]);
        assert!(
            serde_json::from_value::<Operation>(malformed).is_err(),
            "{field}"
        );
    }
}

#[test]
fn asynchronous_java_startup_is_optional_protocol_four_with_exact_unsigned_ids() {
    for wire in [
        json!({"type":"language_start_java_begin","java_executable":"C:\\jdk\\java.exe","distribution":"C:\\JDT 雪","data_directory":"C:\\data"}),
        json!({"type":"language_start_java_poll","startup_id":1}),
        json!({"type":"language_start_java_cancel","startup_id":u64::MAX}),
    ] {
        let operation: Operation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&operation).unwrap(), wire);
        assert!(JAVA_STARTUP_CAPABILITIES.contains(&operation.capability_name().unwrap()));
    }
    for method in ["language_start_java_poll", "language_start_java_cancel"] {
        for id in [json!(-1), json!(1.5), json!("1"), json!(null), json!([])] {
            assert!(
                serde_json::from_value::<Operation>(json!({"type":method,"startup_id":id}))
                    .is_err()
            );
        }
        assert!(serde_json::from_value::<Operation>(json!({"type":method})).is_err());
    }
    for capability in JAVA_STARTUP_CAPABILITIES {
        assert!(!JAVA_LANGUAGE_SESSION_CAPABILITIES.contains(capability));
        assert!(!LANGUAGE_SESSION_CAPABILITIES.contains(capability));
        assert!(!supports_capability(None, capability));
    }
    assert_eq!(PROTOCOL_VERSION, 4);
}
