use cedar_protocol::{
    read_frame, scoped_search_mask_matches, scoped_search_masks_match, write_frame, AgentInfo,
    FileDiscoveryDescriptor, Operation, Payload, Response, ScopedSearchDescriptor,
    ScopedSearchIncompleteReason as Reason, ScopedSearchRequest, ScopedSearchResult, SearchMatch,
};
use serde_json::json;

// Exact accepted 0.61.0 sources, not current aliases or a hand-written old reader.
// Public source commit: 7ec8e2f13ce82c57185c705929c0c17c61fcbdfa.
// lib.rs blob: 89e09e4afbf7e1377374fb44140e4e702361becd.
// file_discovery.rs blob: 9d9b519147b9cc17c65c015ddcfb475d72068af1.
// maven_dependencies.rs blob: 8707970b7d2ab118074b34e5090e654cc8e3212a.
#[allow(dead_code)]
#[path = "fixtures/accepted_061/lib.rs"]
mod accepted_061;
// The unchanged accepted child modules refer to their original crate root.
use accepted_061::RemoteError;

fn agent() -> AgentInfo {
    AgentInfo {
        schema: 1,
        version: "0.61.0".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        capabilities: vec!["list".into(), "read".into()],
        capability_groups: vec![],
    }
}

fn hello(descriptor: Option<ScopedSearchDescriptor>) -> Response {
    Response {
        id: 61,
        result: Ok(Payload::Hello {
            protocol: 4,
            root: "/fixture".into(),
            agent: Some(agent()),
            file_discovery: Some(FileDiscoveryDescriptor::V1),
            scoped_search: descriptor,
            case_search: None,
        }),
    }
}

fn raw_hello(fields: &str) -> String {
    format!("{{\"id\":61,\"result\":{{\"Ok\":{{\"type\":\"hello\",\"protocol\":4,\"root\":\"/fixture\",{fields}}}}}}}\n")
}

fn request() -> ScopedSearchRequest {
    ScopedSearchRequest {
        query: "needle".into(),
        scope: "src".into(),
        masks: vec!["*.rs".into()],
        limit: 100,
    }
}

fn result() -> ScopedSearchResult {
    ScopedSearchResult {
        request: request(),
        matches: vec![
            SearchMatch {
                path: "src/a.rs".into(),
                line: 1,
                text: "needle".into(),
            },
            SearchMatch {
                path: "src/a.rs".into(),
                line: 2,
                text: "another needle".into(),
            },
            SearchMatch {
                path: "src/nested/b.rs".into(),
                line: 1,
                text: "needle".into(),
            },
        ],
        incomplete: false,
        reasons: vec![],
        examined_entries: 3,
        read_bytes: 100,
    }
}

#[test]
fn descriptor_is_one_positive_u32_with_exact_compact_size_and_independent_support() {
    assert_eq!(ScopedSearchDescriptor::V1.version, 1);
    assert_eq!(cedar_protocol::SCOPED_SEARCH_VERSION, 1);
    assert_eq!(cedar_protocol::MAX_SCOPED_SEARCH_DESCRIPTOR_BYTES, 22);
    for version in [1, 2, u32::MAX] {
        let descriptor = ScopedSearchDescriptor { version };
        descriptor.validate().unwrap();
        let bytes = serde_json::to_vec(&descriptor).unwrap();
        assert_eq!(bytes.len(), if version == u32::MAX { 22 } else { 13 });
        assert_eq!(
            serde_json::from_slice::<ScopedSearchDescriptor>(&bytes).unwrap(),
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
    assert!(ScopedSearchDescriptor { version: 0 }.validate().is_err());
    assert_eq!(cedar_protocol::PROTOCOL_VERSION, 4);
    assert_eq!(cedar_protocol::AGENT_INFO_SCHEMA, 1);
    assert_eq!(cedar_protocol::MAX_AGENT_CAPABILITIES, 32);
    assert_eq!(cedar_protocol::MAX_AGENT_CAPABILITY_GROUPS, 2);
}

#[test]
fn malformed_descriptors_and_duplicate_outer_fields_fail_even_with_null_or_escaped_names() {
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
        let raw = raw_hello(&format!("\"scoped_search\":{value}"));
        assert!(
            read_frame::<_, Response>(&mut raw.as_bytes()).is_err(),
            "accepted {value}"
        );
    }
    for first in ["null", "{\"version\":1}", "{\"version\":2}"] {
        for second in ["null", "{\"version\":1}"] {
            for key in ["scoped_search", "scoped_se\\u0061rch"] {
                let raw = raw_hello(&format!("\"scoped_search\":{first},\"{key}\":{second}"));
                assert!(
                    read_frame::<_, Response>(&mut raw.as_bytes()).is_err(),
                    "accepted {raw}"
                );
            }
        }
    }
    // The 22-byte budget describes compact encoding, not whitespace or escapes.
    let raw = raw_hello("\"scoped_search\": { \"versi\\u006fn\" : 1 }");
    assert!(read_frame::<_, Response>(&mut raw.as_bytes())
        .unwrap()
        .is_some());
}

#[test]
fn absent_null_and_future_scoped_descriptors_do_not_advertise_v1_support() {
    for fields in ["\"agent\":null", "\"scoped_search\":null"] {
        let raw = raw_hello(fields);
        let current: Response = read_frame(&mut raw.as_bytes()).unwrap().unwrap();
        assert!(matches!(
            current.result,
            Ok(Payload::Hello {
                scoped_search: None,
                ..
            })
        ));
    }
    let current: Response = serde_json::from_value(
        serde_json::to_value(hello(Some(ScopedSearchDescriptor { version: u32::MAX }))).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        current.result,
        Ok(Payload::Hello { scoped_search: Some(descriptor), .. }) if !descriptor.supported()
    ));
    let wire = serde_json::to_value(hello(None)).unwrap();
    assert!(wire["result"]["Ok"].get("scoped_search").is_none());
}

#[test]
fn actual_accepted_061_reader_ignores_new_hello_field_but_rejects_new_operation() {
    let mut response = hello(Some(ScopedSearchDescriptor::V1));
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
    let old: accepted_061::Response = accepted_061::read_frame(&mut &bytes[..]).unwrap().unwrap();
    let accepted_061::Payload::Hello {
        protocol,
        root,
        agent: Some(old),
        file_discovery: Some(discovery),
    } = old.result.unwrap()
    else {
        panic!();
    };
    old.validate().unwrap();
    discovery.validate().unwrap();
    assert!(discovery.supported());
    assert_eq!(protocol, 4);
    assert_eq!(root, "/fixture");
    assert_eq!(old.schema, 1);
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
    let mut invalid = old.clone();
    invalid.schema = 2;
    assert!(invalid.validate().is_err());
    for fields in [
        "\"agent\":null,\"agent\":null,\"scoped_search\":{\"version\":1}",
        "\"file_discovery\":null,\"file_discovery\":null,\"scoped_search\":{\"version\":1}",
        "\"file_discovery\":{\"version\":0},\"scoped_search\":{\"version\":1}",
    ] {
        let raw = raw_hello(fields);
        assert!(
            accepted_061::read_frame::<_, accepted_061::Response>(&mut raw.as_bytes()).is_err()
        );
    }
    let operation = Operation::SearchIn { request: request() };
    assert_eq!(operation.capability_name(), None);
    assert!(serde_json::from_value::<accepted_061::Operation>(
        serde_json::to_value(operation).unwrap()
    )
    .is_err());
    let old_hello = accepted_061::Response {
        id: 62,
        result: Ok(accepted_061::Payload::Hello {
            protocol: 4,
            root: "/old".into(),
            agent: Some(old),
            file_discovery: Some(discovery),
        }),
    };
    bytes.clear();
    accepted_061::write_frame(&mut bytes, &old_hello).unwrap();
    let current: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
    assert!(matches!(
        current.result,
        Ok(Payload::Hello {
            scoped_search: None,
            case_search: None,
            file_discovery: Some(_),
            ..
        })
    ));
}

#[test]
fn request_wire_is_strict_and_preserves_exact_text_scope_masks_and_limit() {
    let mut expected = request();
    expected.query = "  needle\t雪*?  ".into();
    expected.scope = "src/项目".into();
    expected.masks = vec!["*.rs".into(), "?雪.*".into()];
    expected.limit = 17;
    expected.validate().unwrap();
    let wire = json!({"type":"search_in", "request":expected});
    let decoded: Operation = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
    assert!(matches!(decoded, Operation::SearchIn { request } if request == expected));
    for field in ["query", "scope", "masks", "limit"] {
        let mut invalid = serde_json::to_value(request()).unwrap();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ScopedSearchRequest>(invalid).is_err());
        let mut invalid = serde_json::to_value(request()).unwrap();
        invalid[field] = serde_json::Value::Null;
        assert!(serde_json::from_value::<ScopedSearchRequest>(invalid).is_err());
    }
    for extra in ["regex", "case_sensitive", "follow_symlinks", "extra"] {
        let mut invalid = serde_json::to_value(request()).unwrap();
        invalid[extra] = json!(false);
        assert!(serde_json::from_value::<ScopedSearchRequest>(invalid).is_err());
    }
    for suffix in [
        ",\"query\":\"other\"",
        ",\"qu\\u0065ry\":\"other\"",
        ",\"scope\":null",
        ",\"masks\":[]",
        ",\"limit\":1",
    ] {
        let mut raw = serde_json::to_string(&request()).unwrap();
        raw.pop();
        raw.push_str(suffix);
        raw.push('}');
        assert!(serde_json::from_str::<ScopedSearchRequest>(&raw).is_err());
    }
    for masks in [json!("*.rs"), json!([null]), json!([1]), json!({})] {
        let mut invalid = serde_json::to_value(request()).unwrap();
        invalid["masks"] = masks;
        assert!(serde_json::from_value::<ScopedSearchRequest>(invalid).is_err());
    }
    for limit in [json!(-1), json!(1.5), json!("1"), json!(true)] {
        let mut invalid = serde_json::to_value(request()).unwrap();
        invalid["limit"] = limit;
        assert!(serde_json::from_value::<ScopedSearchRequest>(invalid).is_err());
    }
}

#[test]
fn request_validation_bounds_literal_query_exact_scope_masks_and_effective_limit() {
    let mut valid = request();
    valid.query = "é".repeat(2048);
    valid.scope = "a".repeat(4096);
    valid.masks = vec!["a".repeat(32); 8];
    valid.limit = 1000;
    assert_eq!(valid.validate().unwrap(), 1000);
    valid.limit = usize::MAX;
    assert_eq!(valid.validate().unwrap(), 1000);
    for query in [
        "".into(),
        "a\nb".into(),
        "a\rb".into(),
        "a\0b".into(),
        "é".repeat(2049),
    ] {
        let mut invalid = request();
        invalid.query = query;
        assert!(invalid.validate().is_err(), "{:?}", invalid.query);
    }
    for scope in [
        "/src",
        "src/",
        "src//nested",
        "src/./nested",
        "src/../nested",
        "..",
        ".",
        "C:/src",
        "src\\nested",
        "src\nother",
        "target",
        "src/.git",
        "node_modules/pkg",
        "src/build/pkg",
        "src/dist/pkg",
        "src/.idea/pkg",
        "src/.venv/pkg",
        "src/venv/pkg",
        "src/__pycache__/pkg",
    ] {
        let mut invalid = request();
        invalid.scope = scope.into();
        assert!(invalid.validate().is_err(), "{scope:?}");
    }
    let mut invalid = request();
    invalid.scope = "a".repeat(4097);
    assert!(invalid.validate().is_err());
    let mut valid = request();
    valid.scope.clear();
    valid.masks.clear();
    valid.validate().unwrap();
    for mask in [
        "", "a/b", "a\\b", "a\nb", "a\rb", "a\tb", "a\0b", "a\u{7f}b", "a;b", "[ab]", "{a,b}",
        "!*.rs",
    ] {
        let mut invalid = request();
        invalid.masks = vec![mask.into()];
        assert!(invalid.validate().is_err(), "{mask:?}");
    }
    for masks in [
        vec!["*".into(); 9],
        vec!["a".repeat(257)],
        vec!["é".repeat(129)],
    ] {
        let mut invalid = request();
        invalid.masks = masks;
        assert!(invalid.validate().is_err());
    }
    let mut invalid = request();
    invalid.limit = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn masks_use_only_literal_case_sensitive_basename_globs_over_unicode_scalars() {
    for (mask, basename) in [
        ("*", ""),
        ("*", "雪.rs"),
        ("**a***?", "a雪"),
        ("*a", "*ba"),
        ("a*b", "a*b"),
        ("*?", "*雪"),
        ("?.rs", "雪.rs"),
        ("?", "é"),
        ("?", "😀"),
        ("??", "e\u{301}"),
        ("a*b*c", "axybz c"),
        ("file.(rs)", "file.(rs)"),
        ("*.rs", "main.rs"),
    ] {
        assert!(
            scoped_search_mask_matches(mask, basename),
            "{mask:?} {basename:?}"
        );
    }
    for (mask, basename) in [
        ("?", ""),
        ("?", "e\u{301}"),
        ("??", "😀"),
        ("?.rs", "ab.rs"),
        ("a*b", "ac"),
        ("*.rs", "MAIN.RS"),
        ("é", "e\u{301}"),
        ("file.(rs)", "file.rs"),
        ("Straße", "STRASSE"),
    ] {
        assert!(
            !scoped_search_mask_matches(mask, basename),
            "{mask:?} {basename:?}"
        );
    }
    assert!(scoped_search_masks_match(&[], "anything.java"));
    let masks = vec!["*.rs".into(), "?.java".into()];
    assert!(scoped_search_masks_match(&masks, "main.rs"));
    assert!(scoped_search_masks_match(&masks, "雪.java"));
    assert!(!scoped_search_masks_match(&masks, "main.java"));
}

#[test]
fn result_round_trip_is_strict_and_allows_multiple_lines_from_one_examined_file() {
    let expected = result();
    expected.validate_for(&request()).unwrap();
    let response = Response {
        id: 63,
        result: Ok(Payload::ScopedMatches { result: expected }),
    };
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &response).unwrap();
    let decoded: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
    let Payload::ScopedMatches { result: decoded } = decoded.result.unwrap() else {
        panic!();
    };
    decoded.validate_for(&request()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(result()).unwrap()
    );
    let mut one_file = result();
    one_file.matches.pop();
    one_file.examined_entries = 1;
    one_file.validate_for(&request()).unwrap();
    for field in [
        "request",
        "matches",
        "incomplete",
        "reasons",
        "examined_entries",
        "read_bytes",
    ] {
        let mut invalid = serde_json::to_value(result()).unwrap();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ScopedSearchResult>(invalid).is_err());
    }
    let mut invalid = serde_json::to_value(result()).unwrap();
    invalid["extra"] = json!(null);
    assert!(serde_json::from_value::<ScopedSearchResult>(invalid).is_err());
    let mut raw = serde_json::to_string(&result()).unwrap();
    raw.pop();
    raw.push_str(",\"read_byt\\u0065s\":0}");
    assert!(serde_json::from_str::<ScopedSearchResult>(&raw).is_err());
    let mut invalid = serde_json::to_value(result()).unwrap();
    invalid["reasons"] = json!(["future_reason"]);
    assert!(serde_json::from_value::<ScopedSearchResult>(invalid).is_err());
}

#[test]
fn results_reject_echo_membership_order_snippet_count_and_completeness_errors() {
    type Mutation = Box<dyn Fn(&mut ScopedSearchResult)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|r| r.request.query = "other".into()),
        Box::new(|r| r.request.scope.clear()),
        Box::new(|r| r.request.masks.clear()),
        Box::new(|r| r.request.limit += 1),
        Box::new(|r| r.matches[0].path = "src-other/a.rs".into()),
        Box::new(|r| r.matches[0].path = "src".into()),
        Box::new(|r| r.matches[0].path = "src/../a.rs".into()),
        Box::new(|r| r.matches[0].path = "src//a.rs".into()),
        Box::new(|r| r.matches[0].path = "src\\a.rs".into()),
        Box::new(|r| r.matches[0].path = "src/a.txt".into()),
        Box::new(|r| r.matches[0].path = "src/a.rs/does-not-match".into()),
        Box::new(|r| r.matches[0].path = "src/target/a.rs".into()),
        Box::new(|r| r.matches[0].path = format!("src/{}.rs", "a".repeat(4096))),
        Box::new(|r| r.matches.swap(0, 1)),
        Box::new(|r| r.matches.swap(0, 2)),
        Box::new(|r| r.matches[1] = r.matches[0].clone()),
        Box::new(|r| r.matches[0].line = 0),
        Box::new(|r| r.matches[0].line = cedar_protocol::MAX_FILE_BYTES + 1),
        Box::new(|r| r.matches[0].text = "a".repeat(2055)),
        Box::new(|r| r.matches[0].text = "é".repeat(1028)),
        Box::new(|r| r.matches[0].text = "needle\n".into()),
        Box::new(|r| r.matches[0].text = "needle\0".into()),
        Box::new(|r| r.incomplete = true),
        Box::new(|r| r.reasons.push(Reason::TimeLimit)),
        Box::new(|r| r.examined_entries = 20_001),
        Box::new(|r| r.examined_entries = 0),
        Box::new(|r| r.read_bytes = 32 * 1024 * 1024 + 1),
        Box::new(|r| r.read_bytes = 0),
        Box::new(|r| {
            r.incomplete = true;
            r.reasons = vec![Reason::ReadByteLimit; 2];
        }),
    ];
    for mutate in mutations {
        let mut invalid = result();
        mutate(&mut invalid);
        assert!(invalid.validate_for(&request()).is_err(), "{invalid:?}");
    }
    let mut too_many = result();
    too_many.request.limit = 2;
    assert!(too_many.validate_for(&too_many.request).is_err());
    let mut invalid_request = request();
    invalid_request.query.clear();
    let mut invalid = result();
    invalid.request = invalid_request.clone();
    assert!(invalid.validate_for(&invalid_request).is_err());
}

#[test]
fn results_accept_exact_bounds_each_reason_and_depth_relative_to_scope() {
    assert_eq!(cedar_protocol::MAX_SCOPED_SEARCH_RESULTS, 1000);
    assert_eq!(cedar_protocol::MAX_SCOPED_SEARCH_ENTRIES, 20_000);
    assert_eq!(cedar_protocol::MAX_SCOPED_SEARCH_DEPTH, 64);
    assert_eq!(
        cedar_protocol::MAX_SCOPED_SEARCH_READ_BYTES,
        32 * 1024 * 1024
    );
    assert_eq!(cedar_protocol::MAX_SCOPED_SEARCH_RESULT_BYTES, 512 * 1024);
    assert_eq!(cedar_protocol::MAX_SCOPED_SEARCH_SNIPPET_BYTES, 2054);
    assert_eq!(Reason::COUNT, 11);
    let mut bounded = result();
    bounded.matches.truncate(1);
    bounded.matches[0].line = cedar_protocol::MAX_FILE_BYTES;
    bounded.matches[0].text = "é".repeat(1027);
    bounded.matches[0].path = format!("src/{}.rs", "a".repeat(4089));
    assert_eq!(bounded.matches[0].path.len(), 4096);
    bounded.examined_entries = 20_000;
    bounded.read_bytes = 32 * 1024 * 1024;
    bounded.validate_for(&request()).unwrap();
    for reason in [
        Reason::PermissionDenied,
        Reason::IoError,
        Reason::DepthLimit,
        Reason::TimeLimit,
        Reason::EntryLimit,
        Reason::StackLimit,
        Reason::ResponseLimit,
        Reason::ResultLimit,
        Reason::UnrepresentablePath,
        Reason::UnsafePath,
        Reason::ReadByteLimit,
    ] {
        let mut partial = result();
        partial.matches.clear();
        partial.incomplete = true;
        partial.reasons = vec![reason];
        partial.validate_for(&request()).unwrap();
    }
    let mut bounded = result();
    bounded.matches.truncate(1);
    bounded.matches[0].path = format!("src/{}a.rs", "d/".repeat(64));
    bounded.validate_for(&request()).unwrap();
    bounded.matches[0].path = format!("src/{}a.rs", "d/".repeat(65));
    assert!(bounded.validate_for(&request()).is_err());
    // Legacy line previews can contain an interior CR; only LF and NUL are
    // rejected at this boundary. Preserve those literal bytes on round trip.
    let mut interior_cr = result();
    interior_cr.matches[0].text = "needle\rstill this line".into();
    interior_cr.validate_for(&request()).unwrap();
    let round_trip: ScopedSearchResult =
        serde_json::from_slice(&serde_json::to_vec(&interior_cr).unwrap()).unwrap();
    assert_eq!(round_trip.matches[0].text, interior_cr.matches[0].text);
}

#[test]
fn result_limit_is_capped_without_changing_the_original_echo() {
    let mut bounded = result();
    bounded.request.limit = usize::MAX;
    bounded.matches = (1..=1000)
        .map(|line| SearchMatch {
            path: "src/a.rs".into(),
            line,
            text: "needle".into(),
        })
        .collect();
    bounded.examined_entries = 1;
    bounded.read_bytes = 7007;
    bounded.validate_for(&bounded.request).unwrap();
    let mut altered_echo = bounded.request.clone();
    altered_echo.limit = 1000;
    assert!(bounded.validate_for(&altered_echo).is_err());
    bounded.matches.push(SearchMatch {
        path: "src/a.rs".into(),
        line: 1001,
        text: "needle".into(),
    });
    assert!(bounded.validate_for(&bounded.request).is_err());
}

#[test]
fn compact_result_budget_counts_escaped_paths_and_snippets() {
    for escape_paths in [true, false] {
        let mut oversized = result();
        oversized.request.limit = 1000;
        oversized.matches = (0..200)
            .map(|i| SearchMatch {
                path: if escape_paths {
                    format!("src/a{i:03}{}.rs", "\"".repeat(1800))
                } else {
                    format!("src/a{i:03}.rs")
                },
                line: 1,
                text: if escape_paths {
                    "needle".into()
                } else {
                    "\"".repeat(1800)
                },
            })
            .collect();
        oversized.examined_entries = 200;
        oversized.read_bytes = 400_000;
        let decoded_bytes: usize = oversized
            .matches
            .iter()
            .map(|m| m.path.len() + m.text.len())
            .sum();
        assert!(decoded_bytes < 512 * 1024);
        assert!(serde_json::to_vec(&oversized).unwrap().len() > 512 * 1024);
        assert!(oversized.encoded_bytes().is_err());
        assert!(oversized.validate_for(&oversized.request).is_err());
        oversized.matches.truncate(100);
        let encoded = serde_json::to_vec(&oversized).unwrap();
        assert!(encoded.len() < 512 * 1024);
        assert_eq!(oversized.encoded_bytes().unwrap(), encoded.len());
        oversized.validate_for(&oversized.request).unwrap();
    }
}

#[test]
fn new_scoped_types_require_objects_not_positional_struct_arrays() {
    assert!(serde_json::from_value::<ScopedSearchDescriptor>(json!([1])).is_err());
    assert!(
        serde_json::from_value::<ScopedSearchRequest>(json!(["needle", "src", ["*.rs"], 100]))
            .is_err()
    );
    let valid = result();
    assert!(serde_json::from_value::<ScopedSearchResult>(json!([
        valid.request,
        valid.matches,
        valid.incomplete,
        valid.reasons,
        valid.examined_entries,
        valid.read_bytes
    ]))
    .is_err());
    let raw = raw_hello("\"scoped_search\":[1]");
    assert!(read_frame::<_, Response>(&mut raw.as_bytes()).is_err());
    assert!(serde_json::from_value::<Operation>(json!({
        "type":"search_in", "request":["needle", "src", ["*.rs"], 100]
    }))
    .is_err());
    let mut invalid = serde_json::to_value(result()).unwrap();
    invalid["request"] = json!(["needle", "src", ["*.rs"], 100]);
    assert!(serde_json::from_value::<ScopedSearchResult>(invalid).is_err());
    for value in [
        json!(null),
        json!(true),
        json!("object"),
        json!(1),
        json!([]),
    ] {
        assert!(serde_json::from_value::<ScopedSearchDescriptor>(value.clone()).is_err());
        assert!(serde_json::from_value::<ScopedSearchRequest>(value.clone()).is_err());
        assert!(serde_json::from_value::<ScopedSearchResult>(value).is_err());
    }
    serde_json::from_value::<ScopedSearchDescriptor>(json!({"version":1})).unwrap();
    serde_json::from_value::<ScopedSearchRequest>(serde_json::to_value(request()).unwrap())
        .unwrap();
    serde_json::from_value::<ScopedSearchResult>(serde_json::to_value(result()).unwrap()).unwrap();
}
