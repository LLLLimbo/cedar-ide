//! Case-search contract admission uses the established real-pipe fixture.
//! Stage 2 also verifies the supported embedded backend; mock-peer gates remain independent.
use super::*;
use cedar_protocol::CaseSearchRequest;

fn case_request() -> CaseSearchRequest {
    CaseSearchRequest {
        query: "needle".into(),
        scope: "src".into(),
        masks: vec!["*.rs".into()],
        limit: 100,
        case_sensitive: false,
    }
}

fn case_operation() -> Operation {
    Operation::SearchCase {
        request: case_request(),
    }
}

fn case_hello(descriptor: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "hello", "protocol": PROTOCOL_VERSION, "root": "/fixture",
        "agent": agent_info(&["list", "read", "search_case"]),
        "case_search": descriptor,
    })
}

fn case_result() -> serde_json::Value {
    serde_json::json!({
        "request": case_request(),
        "matches": [{"path": "src/file.rs", "line": 1, "text": "needle"}],
        "incomplete": false, "reasons": [], "examined_entries": 1, "read_bytes": 6,
    })
}

fn case_response(directory: &Path, result: serde_json::Value) {
    fs::write(
        directory.join("response-2.json"),
        serde_json::json!({"Ok": {"type": "case_matches", "result": result}}).to_string(),
    )
    .unwrap();
}

#[test]
fn case_search_absent_null_future_and_flat_claims_send_zero_new_operations() {
    for descriptor in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!({"version": 2})),
        Some(serde_json::json!({"version": u32::MAX})),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut hello = case_hello(descriptor.clone().unwrap_or(serde_json::Value::Null));
        hello["scoped_search"] = serde_json::json!({"version": 1});
        if descriptor.is_none() {
            hello.as_object_mut().unwrap().remove("case_search");
        }
        let mut client = Client::from_process(capability_peer(directory.path(), hello)).unwrap();
        assert!(!client.supports_case_search());
        let snapshot = serde_json::to_value(client.handshake()).unwrap();
        if let Some(version) = descriptor
            .as_ref()
            .and_then(|value| value["version"].as_u64())
        {
            assert_eq!(snapshot["case_search"]["version"], version);
        }
        for case_sensitive in [false, true] {
            let mut request = case_request();
            request.case_sensitive = case_sensitive;
            assert!(client
                .request(Operation::SearchCase { request })
                .unwrap_err()
                .starts_with("unsupported_operation:"));
        }
        assert_eq!(
            serde_json::to_value(client.request(Operation::Hello).unwrap()).unwrap(),
            snapshot
        );
        read_fixture(&mut client);
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["op"]["type"], "hello");
        assert_eq!(requests[1]["op"]["type"], "read");
        assert!(client.is_connected());
        client.close_and_wait(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn case_search_descriptor_alone_enables_exact_request_and_retains_first_hello() {
    for (requested_limit, case_sensitive) in [(100, false), (1001, false), (100, true)] {
        let directory = tempfile::tempdir().unwrap();
        let mut hello = case_hello(serde_json::json!({"version": 1}));
        hello["agent"] = serde_json::json!(agent_info(&["list", "read"]));
        let mut client = Client::from_process(capability_peer(directory.path(), hello)).unwrap();
        assert!(client.supports_case_search());
        let snapshot = serde_json::to_value(client.handshake()).unwrap();
        let mut request = case_request();
        request.limit = requested_limit;
        request.case_sensitive = case_sensitive;
        let mut result = case_result();
        result["request"] = serde_json::to_value(&request).unwrap();
        case_response(directory.path(), result);
        assert!(matches!(
            client.request(Operation::SearchCase { request: request.clone() }).unwrap(),
            Payload::CaseMatches { result } if result.request == request
        ));
        assert_eq!(
            serde_json::to_value(client.request(Operation::Hello).unwrap()).unwrap(),
            snapshot
        );
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["op"],
            serde_json::json!({"type": "search_case", "request": request})
        );
        client.close_and_wait(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn case_search_root_requests_preserve_literal_text_and_accept_cropped_snippets() {
    for case_sensitive in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut hello = case_hello(serde_json::json!({"version": 1}));
        hello["agent"] = serde_json::json!(agent_info(&["list", "read"]));
        let mut client = Client::from_process(capability_peer(directory.path(), hello)).unwrap();
        assert!(!client.supports_scoped_search());
        let request = CaseSearchRequest {
            query: " \t.*[x](?i)^$\\# ".into(),
            scope: String::new(),
            masks: vec![],
            limit: 100,
            case_sensitive,
        };
        let mut result = case_result();
        result["request"] = serde_json::to_value(&request).unwrap();
        result["matches"][0] = serde_json::json!({"path": "file.rs", "line": 1, "text": "cropped"});
        result["read_bytes"] = serde_json::json!(100);
        case_response(directory.path(), result);
        assert!(matches!(
            client.request(Operation::SearchCase { request: request.clone() }).unwrap(),
            Payload::CaseMatches { result } if result.request == request && result.matches[0].text == "cropped"
        ));
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["op"],
            serde_json::json!({"type": "search_case", "request": request})
        );
    }
}

#[test]
fn case_search_handshake_validation_checks_typed_metadata_before_retaining_it() {
    let mut hello: Payload =
        serde_json::from_value(case_hello(serde_json::json!({"version": 1}))).unwrap();
    validate_handshake(&hello).unwrap();
    let Payload::Hello {
        case_search: Some(descriptor),
        ..
    } = &mut hello
    else {
        panic!("fixture descriptor missing");
    };
    descriptor.version = 0;
    assert!(validate_handshake(&hello)
        .unwrap_err()
        .starts_with("invalid_case_search:"));
}

#[test]
fn case_search_malformed_metadata_fails_handshake_without_probing() {
    for descriptor in [
        "{}",
        "{\"version\":0}",
        "{\"version\":-1}",
        "{\"version\":1.0}",
        "{\"version\":4294967296}",
        "{\"version\":null}",
        "{\"version\":true}",
        "{\"version\":\"1\"}",
        "{\"version\":1,\"version\":1}",
        r#"{"version":1,"ver\u0073ion":1}"#,
        "{\"version\":1,\"extra\":1}",
        "[]",
        "[1]",
        "true",
    ] {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("hello.json"), format!("{{\"type\":\"hello\",\"protocol\":4,\"root\":\"/fixture\",\"case_search\":{descriptor}}}")).unwrap();
        let failure = match Client::from_process(spawn("capability_peer", directory.path())) {
            Ok(_) => panic!("accepted malformed descriptor {descriptor}"),
            Err(failure) => failure,
        };
        assert!(failure.starts_with("transport_read:"), "{failure}");
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
    for fields in [
        "\"case_search\":null,\"case_search\":{\"version\":1}",
        "\"case_search\":{\"version\":1},\"case_search\":null",
        r#""case_search":null,"case_search":null"#,
        r#""case_search":null,"case_\u0073earch":{"version":1}"#,
    ] {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("hello.json"),
            format!("{{\"type\":\"hello\",\"protocol\":4,\"root\":\"/fixture\",{fields}}}"),
        )
        .unwrap();
        assert!(Client::from_process(spawn("capability_peer", directory.path())).is_err());
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
}

#[test]
fn case_search_invalid_requests_never_reach_the_transport() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        case_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let mut requests = Vec::new();
    for query in ["", "needle\nneedle", "needle\rneedle", "needle\0"] {
        let mut request = case_request();
        request.query = query.into();
        requests.push(request);
    }
    let mut oversized_query = case_request();
    oversized_query.query = "n".repeat(cedar_protocol::MAX_SCOPED_SEARCH_QUERY_BYTES + 1);
    requests.push(oversized_query);
    for scope in [
        "../src",
        "/src",
        "src\\nested",
        "src/target",
        "node_modules",
    ] {
        let mut request = case_request();
        request.scope = scope.into();
        requests.push(request);
    }
    for masks in [vec![""], vec!["../*.rs"], vec!["src/*.rs"]] {
        let mut request = case_request();
        request.masks = masks.into_iter().map(str::to_owned).collect();
        requests.push(request);
    }
    let mut too_many_masks = case_request();
    too_many_masks.masks = vec!["*.rs".into(); cedar_protocol::MAX_SCOPED_SEARCH_MASKS + 1];
    requests.push(too_many_masks);
    let mut zero = case_request();
    zero.limit = 0;
    requests.push(zero);
    for request in requests {
        assert!(request.validate().is_err(), "invalid fixture: {request:?}");
        assert!(client.request(Operation::SearchCase { request }).is_err());
        assert!(client.is_connected());
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
    read_fixture(&mut client);
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[test]
fn case_search_malformed_results_disconnect_without_exposing_partial_matches() {
    let mut invalids = Vec::new();
    for (field, value) in [
        ("query", serde_json::json!("other")),
        ("scope", serde_json::json!("src/nested")),
        ("masks", serde_json::json!(["*.txt"])),
        ("limit", serde_json::json!(99)),
        ("case_sensitive", serde_json::json!(true)),
    ] {
        let mut result = case_result();
        result["request"][field] = value;
        invalids.push(result);
    }
    for path in [
        "src-other/file.rs",
        "src/file.txt",
        "src/file.RS",
        "src/target/file.rs",
        "src/.git/file.rs",
        "/src/file.rs",
        "src/../file.rs",
        "src\\file.rs",
    ] {
        let mut result = case_result();
        result["matches"][0]["path"] = serde_json::json!(path);
        invalids.push(result);
    }
    for line in [0, cedar_protocol::MAX_FILE_BYTES + 1, usize::MAX] {
        let mut result = case_result();
        result["matches"][0]["line"] = serde_json::json!(line);
        invalids.push(result);
    }
    for text in ["needle\nnext".into(), "needle\0".into(), "n".repeat(2055)] {
        let mut result = case_result();
        result["matches"][0]["text"] = serde_json::json!(text);
        invalids.push(result);
    }
    for matches in [
        serde_json::json!([
            {"path": "src/z.rs", "line": 1, "text": "needle"},
            {"path": "src/a.rs", "line": 1, "text": "needle"},
        ]),
        serde_json::json!([
            {"path": "src/file.rs", "line": 2, "text": "needle"},
            {"path": "src/file.rs", "line": 1, "text": "needle"},
        ]),
        serde_json::json!([
            {"path": "src/file.rs", "line": 1, "text": "needle"},
            {"path": "src/file.rs", "line": 1, "text": "needle"},
        ]),
    ] {
        let mut result = case_result();
        result["matches"] = matches;
        result["examined_entries"] = serde_json::json!(2);
        result["read_bytes"] = serde_json::json!(20);
        invalids.push(result);
    }
    let mut status = case_result();
    status["incomplete"] = serde_json::json!(true);
    invalids.push(status);
    let mut over_limit = case_result();
    over_limit["matches"] = serde_json::json!((1..=101)
        .map(|line| serde_json::json!({"path": "src/file.rs", "line": line, "text": "needle"}))
        .collect::<Vec<_>>());
    over_limit["read_bytes"] = serde_json::json!(1000);
    invalids.push(over_limit);
    for (field, value) in [
        ("examined_entries", 0),
        (
            "examined_entries",
            cedar_protocol::MAX_SCOPED_SEARCH_ENTRIES + 1,
        ),
        ("read_bytes", 0),
        (
            "read_bytes",
            cedar_protocol::MAX_SCOPED_SEARCH_READ_BYTES + 1,
        ),
    ] {
        let mut result = case_result();
        result[field] = serde_json::json!(value);
        invalids.push(result);
    }
    let mut duplicate_reasons = case_result();
    duplicate_reasons["incomplete"] = serde_json::json!(true);
    duplicate_reasons["reasons"] = serde_json::json!(["time_limit", "time_limit"]);
    invalids.push(duplicate_reasons);
    for result in invalids {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            case_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        case_response(directory.path(), result.clone());
        let failure = client.request(case_operation()).unwrap_err();
        assert!(
            failure.starts_with("protocol_error:"),
            "{failure}; {result}"
        );
        assert!(!client.is_connected());
        assert!(client
            .request(case_operation())
            .unwrap_err()
            .starts_with("disconnected:"));
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
}

#[test]
fn case_search_escaped_json_budget_is_checked_before_exposing_matches() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        case_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let mut oversized = case_result();
    oversized["matches"] = serde_json::json!((0..100)
        .map(|index| serde_json::json!({
            "path": format!("src/{index:03}{}.rs", "\"".repeat(3000)),
            "line": 1,
            "text": "needle",
        }))
        .collect::<Vec<_>>());
    oversized["examined_entries"] = serde_json::json!(100);
    oversized["read_bytes"] = serde_json::json!(600);
    let parsed: cedar_protocol::CaseSearchResult =
        serde_json::from_value(oversized.clone()).unwrap();
    assert!(
        parsed
            .matches
            .iter()
            .map(|row| row.path.len() + row.text.len())
            .sum::<usize>()
            < cedar_protocol::MAX_SCOPED_SEARCH_RESULT_BYTES
    );
    assert!(
        serde_json::to_vec(&parsed).unwrap().len() > cedar_protocol::MAX_SCOPED_SEARCH_RESULT_BYTES
    );
    case_response(directory.path(), oversized);
    assert!(client
        .request(case_operation())
        .unwrap_err()
        .starts_with("protocol_error:"));
    assert!(!client.is_connected());
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[test]
fn case_search_valid_incomplete_result_and_interior_carriage_return_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        case_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let mut result = case_result();
    result["matches"][0]["text"] = serde_json::json!("needle\rinside");
    result["incomplete"] = serde_json::json!(true);
    result["reasons"] = serde_json::json!(["read_byte_limit"]);
    result["read_bytes"] = serde_json::json!(cedar_protocol::MAX_SCOPED_SEARCH_READ_BYTES);
    case_response(directory.path(), result);
    let Payload::CaseMatches { result } = client.request(case_operation()).unwrap() else {
        panic!("wrong case-search response");
    };
    assert!(result.incomplete);
    assert_eq!(result.matches[0].text, "needle\rinside");
    assert_eq!(
        result.reasons,
        [cedar_protocol::ScopedSearchIncompleteReason::ReadByteLimit]
    );
    assert!(client.is_connected());
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[test]
fn case_search_wrong_payload_disconnects_without_falling_back_to_legacy_search() {
    let mut scoped_result = case_result();
    scoped_result["request"]
        .as_object_mut()
        .unwrap()
        .remove("case_sensitive");
    for payload in [
        serde_json::json!({"type": "matches", "matches": [], "truncated": false}),
        serde_json::json!({"type": "scoped_matches", "result": scoped_result}),
        case_hello(serde_json::json!({"version": 1})),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            case_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        fs::write(
            directory.path().join("response-2.json"),
            serde_json::json!({"Ok": payload}).to_string(),
        )
        .unwrap();
        assert!(client
            .request(case_operation())
            .unwrap_err()
            .starts_with("protocol_error:"));
        assert!(!client.is_connected());
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
}

#[test]
fn case_search_late_hello_cannot_upgrade_and_reconnection_refreshes_support() {
    let old_root = tempfile::tempdir().unwrap();
    let mut old = capability_client(old_root.path(), Some(agent_info(&["list", "read"])));
    let snapshot = serde_json::to_value(old.handshake()).unwrap();
    fs::write(
        old_root.path().join("response-2.json"),
        serde_json::json!({"Ok": case_hello(serde_json::json!({"version": 1}))}).to_string(),
    )
    .unwrap();
    assert!(
        matches!(old.request(Operation::Read { path: "fixture.txt".into() }).unwrap(), Payload::Hello { case_search: Some(descriptor), .. } if descriptor.supported())
    );
    assert!(!old.supports_case_search());
    assert_eq!(serde_json::to_value(old.handshake()).unwrap(), snapshot);
    assert!(old
        .request(case_operation())
        .unwrap_err()
        .starts_with("unsupported_operation:"));
    old.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(old_root.path()).len(), 2);

    let fresh_root = tempfile::tempdir().unwrap();
    let mut fresh = Client::from_process(capability_peer(
        fresh_root.path(),
        case_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    assert!(fresh.supports_case_search());
    case_response(fresh_root.path(), case_result());
    assert!(matches!(
        fresh.request(case_operation()).unwrap(),
        Payload::CaseMatches { .. }
    ));
    fresh.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(fresh_root.path()).len(), 2);

    let downgrade_root = tempfile::tempdir().unwrap();
    let mut downgrade =
        capability_client(downgrade_root.path(), Some(agent_info(&["list", "read"])));
    assert!(!downgrade.supports_case_search());
    assert!(downgrade
        .request(case_operation())
        .unwrap_err()
        .starts_with("unsupported_operation:"));
    downgrade.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(downgrade_root.path()).len(), 1);
}

#[test]
fn case_search_ordered_masks_and_original_requested_limit_are_exact_echoes() {
    for masks in [vec!["*.rs", "*.txt"], vec!["*.txt", "*.rs"]] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            case_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        let mut request = case_request();
        request.limit = 1001;
        request.masks = vec!["*.rs".into(), "*.txt".into()];
        let mut result = case_result();
        result["request"] = serde_json::to_value(&request).unwrap();
        result["request"]["masks"] = serde_json::json!(masks);
        if masks == ["*.rs", "*.txt"] {
            result["request"]["limit"] = serde_json::json!(1000);
        }
        case_response(directory.path(), result);
        let failure = client
            .request(Operation::SearchCase { request })
            .unwrap_err();
        assert!(failure.starts_with("protocol_error:"), "{failure}");
        assert!(!client.is_connected());
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["op"]["type"], "search_case");
    }
}

#[test]
fn case_search_late_hello_cannot_downgrade_the_first_supported_descriptor() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        case_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let snapshot = serde_json::to_value(client.handshake()).unwrap();
    fs::write(
        directory.path().join("response-2.json"),
        serde_json::json!({"Ok": case_hello(serde_json::Value::Null)}).to_string(),
    )
    .unwrap();
    assert!(matches!(
        client
            .request(Operation::Read {
                path: "fixture.txt".into()
            })
            .unwrap(),
        Payload::Hello {
            case_search: None,
            ..
        }
    ));
    assert!(client.supports_case_search());
    assert_eq!(serde_json::to_value(client.handshake()).unwrap(), snapshot);
    fs::write(
        directory.path().join("response-3.json"),
        serde_json::json!({"Ok": {"type": "case_matches", "result": case_result()}}).to_string(),
    )
    .unwrap();
    assert!(matches!(
        client.request(case_operation()).unwrap(),
        Payload::CaseMatches { .. }
    ));
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["op"]["type"], "read");
    assert_eq!(requests[2]["op"]["type"], "search_case");
}

#[test]
fn case_search_strict_wire_shapes_fail_closed_before_row_exposure() {
    let valid = case_result();
    let mut invalids = Vec::new();
    for field in [
        "request",
        "matches",
        "incomplete",
        "reasons",
        "examined_entries",
        "read_bytes",
    ] {
        let mut result = valid.clone();
        result.as_object_mut().unwrap().remove(field);
        invalids.push(result.to_string());
        let mut result = valid.clone();
        result[field] = serde_json::Value::Null;
        invalids.push(result.to_string());
    }
    for field in ["query", "scope", "masks", "limit", "case_sensitive"] {
        let mut result = valid.clone();
        result["request"].as_object_mut().unwrap().remove(field);
        invalids.push(result.to_string());
        let mut result = valid.clone();
        result["request"][field] = serde_json::Value::Null;
        invalids.push(result.to_string());
    }
    for field in ["path", "line", "text"] {
        let mut result = valid.clone();
        result["matches"][0].as_object_mut().unwrap().remove(field);
        invalids.push(result.to_string());
        let mut result = valid.clone();
        result["matches"][0][field] = serde_json::Value::Null;
        invalids.push(result.to_string());
    }
    for target in ["result", "request", "row"] {
        let mut result = valid.clone();
        let object = match target {
            "result" => &mut result,
            "request" => &mut result["request"],
            "row" => &mut result["matches"][0],
            _ => unreachable!(),
        };
        object["unknown"] = serde_json::json!(false);
        invalids.push(result.to_string());
    }
    for flag in [
        serde_json::json!(0),
        serde_json::json!("false"),
        serde_json::json!([]),
    ] {
        let mut result = valid.clone();
        result["request"]["case_sensitive"] = flag;
        invalids.push(result.to_string());
    }
    let mut row_array = valid.clone();
    row_array["matches"][0] = serde_json::json!(["src/file.rs", 1, "needle"]);
    invalids.push(row_array.to_string());
    let mut request_array = valid.clone();
    request_array["request"] = serde_json::json!(["needle", "src", ["*.rs"], 100, false]);
    invalids.push(request_array.to_string());
    invalids
        .push(serde_json::json!([valid["request"], valid["matches"], false, [], 1, 6]).to_string());
    let mut unknown_reason = valid.clone();
    unknown_reason["incomplete"] = serde_json::json!(true);
    unknown_reason["reasons"] = serde_json::json!(["future_reason"]);
    invalids.push(unknown_reason.to_string());
    for (needle, replacement) in [
        (
            r#""incomplete":false"#,
            r#""incomplete":false,"incomplete":false"#,
        ),
        (
            r#""query":"needle""#,
            r#""query":"needle","qu\u0065ry":"needle""#,
        ),
        (
            r#""case_sensitive":false"#,
            r#""case_sensitive":false,"case_sensitive":false"#,
        ),
        (
            r#""path":"src/file.rs""#,
            r#""path":"src/file.rs","pa\u0074h":"src/file.rs""#,
        ),
        (r#""line":1"#, r#""line":1,"line":1"#),
        (r#""text":"needle""#, r#""text":"needle","text":"needle""#),
    ] {
        let raw = valid.to_string();
        assert!(raw.contains(needle));
        invalids.push(raw.replacen(needle, replacement, 1));
    }
    for result in invalids {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            case_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        fs::write(
            directory.path().join("response-2.json"),
            format!("{{\"Ok\":{{\"type\":\"case_matches\",\"result\":{result}}}}}"),
        )
        .unwrap();
        let failure = client.request(case_operation()).unwrap_err();
        assert!(
            failure.starts_with("transport_read:"),
            "{failure}; {result}"
        );
        assert!(!client.is_connected());
        assert!(client
            .request(case_operation())
            .unwrap_err()
            .starts_with("disconnected:"));
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["op"]["type"], "search_case");
    }
}

#[test]
fn case_search_ordinary_errors_preserve_connection_without_retry_or_fallback() {
    for code in [
        "case_search_matcher_limit",
        "case_search_matcher_error",
        "unsupported_operation",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            case_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        fs::write(
            directory.path().join("response-2.json"),
            serde_json::json!({"Err": {"code": code, "message": "fixture error"}}).to_string(),
        )
        .unwrap();
        assert_eq!(
            client.request(case_operation()).unwrap_err(),
            format!("{code}: fixture error")
        );
        assert!(client.is_connected());
        assert!(client.supports_case_search());
        read_fixture(&mut client);
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1]["op"]["type"], "search_case");
        assert_eq!(requests[2]["op"]["type"], "read");
    }
}

#[test]
fn case_search_connection_cancellation_interrupts_the_wait_and_reaps_the_owned_child() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("hello.json"),
        case_hello(serde_json::json!({"version": 1})).to_string(),
    )
    .unwrap();
    let token = ConnectionCancellation::new();
    let mut client =
        Client::from_process(cancellable_peer("stalled", directory.path(), token.clone())).unwrap();
    assert_eq!(
        process(&mut client).request_timeout(&case_operation()),
        Duration::from_secs(30)
    );
    let (finished, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        let outcome = client.request(case_operation());
        finished.send((client, outcome)).unwrap();
    });
    wait_until(|| {
        request_file_summary(directory.path()).is_ok_and(|summary| summary.complete_lines == 2)
    });
    let start = Instant::now();
    token.cancel();
    let (client, outcome) = result.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(outcome.unwrap_err().starts_with("transport_cancelled:"));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!client.is_connected());
    caller.join().unwrap();
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert!(directory.path().join("eof").is_file());
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["op"]["type"], "search_case");
}

#[test]
fn case_search_cancellation_polling_preserves_the_original_transport_deadline() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("hello.json"),
        case_hello(serde_json::json!({"version": 1})).to_string(),
    )
    .unwrap();
    let token = ConnectionCancellation::new();
    let mut client =
        Client::from_process(cancellable_peer("stalled", directory.path(), token.clone())).unwrap();
    assert_eq!(
        process(&mut client).request_timeout(&case_operation()),
        Duration::from_secs(30)
    );
    let (finished, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        let start = Instant::now();
        let outcome =
            process(&mut client).request_with_timeout(case_operation(), Duration::from_millis(250));
        finished.send((client, outcome, start.elapsed())).unwrap();
    });
    wait_until(|| {
        request_file_summary(directory.path()).is_ok_and(|summary| summary.complete_lines == 2)
    });
    let (client, outcome, elapsed) = result.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(outcome.unwrap_err().starts_with("transport_timeout:"));
    assert!(
        elapsed >= Duration::from_millis(250),
        "poll slice shortened deadline: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "poll slice renewed deadline: {elapsed:?}"
    );
    assert!(!token.is_cancelled());
    assert!(!client.is_connected());
    caller.join().unwrap();
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["op"]["type"], "search_case");
}

#[cfg(not(windows))]
#[test]
fn case_search_stage_two_embedded_client_negotiates_and_validates_both_modes() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("fixture.txt"), "needle\nNEEDLE\n").unwrap();
    let mut client = Client::connect(ConnectionSpec::Local {
        root: directory.path().to_owned(),
        allow_run: false,
    })
    .unwrap();
    assert!(client.supports_case_search());
    assert!(matches!(
        client.handshake(),
        Payload::Hello {
            case_search: Some(descriptor),
            ..
        } if descriptor.supported()
    ));
    for case_sensitive in [false, true] {
        let request = cedar_protocol::CaseSearchRequest {
            query: "needle".into(),
            scope: String::new(),
            masks: Vec::new(),
            limit: 500,
            case_sensitive,
        };
        let Payload::CaseMatches { result } = client
            .request(Operation::SearchCase {
                request: request.clone(),
            })
            .unwrap()
        else {
            panic!("case result");
        };
        result.validate_for(&request).unwrap();
        assert_eq!(result.matches.len(), if case_sensitive { 1 } else { 2 });
        assert!(!result.incomplete);
    }
    assert!(client.is_connected());
    assert!(
        matches!(client.request(Operation::Read { path: "fixture.txt".into() }).unwrap(), Payload::File { text, .. } if text == "needle\nNEEDLE\n")
    );
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}
