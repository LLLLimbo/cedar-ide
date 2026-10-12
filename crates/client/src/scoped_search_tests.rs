//! Scoped-search checks share the established real-pipe peer without extending
//! its accepted fixture source or enabling this operation through flat claims.
use super::*;
use cedar_protocol::ScopedSearchRequest;

fn scoped_request() -> ScopedSearchRequest {
    ScopedSearchRequest {
        query: "needle".into(),
        scope: "src".into(),
        masks: vec!["*.rs".into()],
        limit: 100,
    }
}

fn scoped_operation() -> Operation {
    Operation::SearchIn {
        request: scoped_request(),
    }
}

fn scoped_hello(descriptor: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "hello", "protocol": PROTOCOL_VERSION, "root": "/fixture",
        "agent": agent_info(&["list", "read", "search_in"]),
        "scoped_search": descriptor,
    })
}

fn scoped_result() -> serde_json::Value {
    serde_json::json!({
        "request": scoped_request(),
        "matches": [{"path": "src/file.rs", "line": 1, "text": "needle"}],
        "incomplete": false, "reasons": [], "examined_entries": 1, "read_bytes": 6,
    })
}

fn scoped_response(directory: &Path, result: serde_json::Value) {
    fs::write(
        directory.join("response-2.json"),
        serde_json::json!({"Ok": {"type": "scoped_matches", "result": result}}).to_string(),
    )
    .unwrap();
}

#[cfg(not(windows))]
#[test]
fn scoped_search_embedded_client_uses_exact_scope_and_masks_without_execution_trust() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("src/nested")).unwrap();
    fs::create_dir_all(directory.path().join("src-other")).unwrap();
    for (path, text) in [
        ("src/file.rs", "needle\n"),
        ("src/nested/雪.rs", "first\nneedle\n"),
        ("src/ignored.txt", "needle\n"),
        ("src-other/file.rs", "needle\n"),
    ] {
        fs::write(directory.path().join(path), text).unwrap();
    }
    let mut client = Client::connect(ConnectionSpec::Local {
        root: directory.path().to_owned(),
        allow_run: false,
    })
    .unwrap();
    assert!(client.supports_scoped_search());
    let request = scoped_request();
    let Payload::ScopedMatches { result } = client
        .request(Operation::SearchIn {
            request: request.clone(),
        })
        .unwrap()
    else {
        panic!("wrong scoped-search response");
    };
    assert_eq!(result.request, request);
    assert_eq!(
        result
            .matches
            .iter()
            .map(|row| (row.path.as_str(), row.line))
            .collect::<Vec<_>>(),
        [("src/file.rs", 1), ("src/nested/雪.rs", 2)]
    );
    assert!(!result.incomplete);
    assert!(result.reasons.is_empty());
    assert!(client.is_connected());
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
fn scoped_search_absent_null_future_and_flat_claims_send_zero_new_operations() {
    for descriptor in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!({"version": 2})),
        Some(serde_json::json!({"version": u32::MAX})),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut hello = scoped_hello(descriptor.clone().unwrap_or(serde_json::Value::Null));
        if descriptor.is_none() {
            hello.as_object_mut().unwrap().remove("scoped_search");
        }
        let mut client = Client::from_process(capability_peer(directory.path(), hello)).unwrap();
        assert!(!client.supports_scoped_search());
        let snapshot = serde_json::to_value(client.handshake()).unwrap();
        assert!(client
            .request(scoped_operation())
            .unwrap_err()
            .starts_with("unsupported_operation:"));
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
fn scoped_search_descriptor_alone_enables_exact_request_and_retains_first_hello() {
    for requested_limit in [100, 1001] {
        let directory = tempfile::tempdir().unwrap();
        let mut hello = scoped_hello(serde_json::json!({"version": 1}));
        hello["agent"] = serde_json::json!(agent_info(&["list", "read"]));
        let mut client = Client::from_process(capability_peer(directory.path(), hello)).unwrap();
        assert!(client.supports_scoped_search());
        let snapshot = serde_json::to_value(client.handshake()).unwrap();
        let mut request = scoped_request();
        request.limit = requested_limit;
        let mut result = scoped_result();
        result["request"] = serde_json::to_value(&request).unwrap();
        scoped_response(directory.path(), result);
        assert!(matches!(
            client.request(Operation::SearchIn { request: request.clone() }).unwrap(),
            Payload::ScopedMatches { result } if result.request == request
        ));
        assert_eq!(
            serde_json::to_value(client.request(Operation::Hello).unwrap()).unwrap(),
            snapshot
        );
        let requests = recorded_requests(directory.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["op"],
            serde_json::json!({"type": "search_in", "request": request})
        );
        client.close_and_wait(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn scoped_search_malformed_metadata_fails_handshake_without_probing() {
    for descriptor in [
        "{}",
        "{\"version\":0}",
        "{\"version\":-1}",
        "{\"version\":1.0}",
        "{\"version\":\"1\"}",
        "{\"version\":1,\"version\":1}",
        "{\"version\":1,\"extra\":1}",
        "[]",
        "[1]",
        "true",
    ] {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("hello.json"), format!("{{\"type\":\"hello\",\"protocol\":4,\"root\":\"/fixture\",\"scoped_search\":{descriptor}}}")).unwrap();
        let failure = match Client::from_process(spawn("capability_peer", directory.path())) {
            Ok(_) => panic!("accepted malformed descriptor {descriptor}"),
            Err(failure) => failure,
        };
        assert!(failure.starts_with("transport_read:"), "{failure}");
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
    for fields in [
        "\"scoped_search\":null,\"scoped_search\":{\"version\":1}",
        "\"scoped_search\":{\"version\":1},\"scoped_search\":null",
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
fn scoped_search_invalid_requests_never_reach_the_transport() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        scoped_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let mut requests = Vec::new();
    for query in ["", "needle\nneedle", "needle\0"] {
        let mut request = scoped_request();
        request.query = query.into();
        requests.push(request);
    }
    let mut oversized_query = scoped_request();
    oversized_query.query = "n".repeat(cedar_protocol::MAX_SCOPED_SEARCH_QUERY_BYTES + 1);
    requests.push(oversized_query);
    for scope in ["../src", "/src", "src\\nested"] {
        let mut request = scoped_request();
        request.scope = scope.into();
        requests.push(request);
    }
    for masks in [vec![""], vec!["../*.rs"], vec!["src/*.rs"]] {
        let mut request = scoped_request();
        request.masks = masks.into_iter().map(str::to_owned).collect();
        requests.push(request);
    }
    let mut too_many_masks = scoped_request();
    too_many_masks.masks = vec!["*.rs".into(); cedar_protocol::MAX_SCOPED_SEARCH_MASKS + 1];
    requests.push(too_many_masks);
    let mut zero = scoped_request();
    zero.limit = 0;
    requests.push(zero);
    for request in requests {
        assert!(request.validate().is_err(), "invalid fixture: {request:?}");
        assert!(client.request(Operation::SearchIn { request }).is_err());
        assert!(client.is_connected());
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
    read_fixture(&mut client);
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[test]
fn scoped_search_malformed_results_disconnect_without_exposing_partial_matches() {
    let mut invalids = Vec::new();
    for (field, value) in [
        ("query", serde_json::json!("other")),
        ("scope", serde_json::json!("src/nested")),
        ("masks", serde_json::json!(["*.txt"])),
        ("limit", serde_json::json!(99)),
    ] {
        let mut result = scoped_result();
        result["request"][field] = value;
        invalids.push(result);
    }
    for path in [
        "src-other/file.rs",
        "src/file.txt",
        "/src/file.rs",
        "src/../file.rs",
        "src\\file.rs",
    ] {
        let mut result = scoped_result();
        result["matches"][0]["path"] = serde_json::json!(path);
        invalids.push(result);
    }
    for line in [0, u64::MAX] {
        let mut result = scoped_result();
        result["matches"][0]["line"] = serde_json::json!(line);
        invalids.push(result);
    }
    for text in ["needle\nnext".into(), "needle\0".into(), "n".repeat(2055)] {
        let mut result = scoped_result();
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
        let mut result = scoped_result();
        result["matches"] = matches;
        result["examined_entries"] = serde_json::json!(2);
        result["read_bytes"] = serde_json::json!(20);
        invalids.push(result);
    }
    let mut status = scoped_result();
    status["incomplete"] = serde_json::json!(true);
    invalids.push(status);
    let mut over_limit = scoped_result();
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
        let mut result = scoped_result();
        result[field] = serde_json::json!(value);
        invalids.push(result);
    }
    let mut duplicate_reasons = scoped_result();
    duplicate_reasons["incomplete"] = serde_json::json!(true);
    duplicate_reasons["reasons"] = serde_json::json!(["time_limit", "time_limit"]);
    invalids.push(duplicate_reasons);
    for result in invalids {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            scoped_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        scoped_response(directory.path(), result.clone());
        let failure = client.request(scoped_operation()).unwrap_err();
        assert!(
            failure.starts_with("protocol_error:"),
            "{failure}; {result}"
        );
        assert!(!client.is_connected());
        assert!(client
            .request(scoped_operation())
            .unwrap_err()
            .starts_with("disconnected:"));
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
}

#[test]
fn scoped_search_escaped_json_budget_is_checked_before_exposing_matches() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        scoped_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let mut oversized = scoped_result();
    oversized["matches"] = serde_json::json!((0..100)
        .map(|index| serde_json::json!({
            "path": format!("src/{index:03}{}.rs", "\"".repeat(3000)),
            "line": 1,
            "text": "needle",
        }))
        .collect::<Vec<_>>());
    oversized["examined_entries"] = serde_json::json!(100);
    oversized["read_bytes"] = serde_json::json!(600);
    let parsed: cedar_protocol::ScopedSearchResult =
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
    scoped_response(directory.path(), oversized);
    assert!(client
        .request(scoped_operation())
        .unwrap_err()
        .starts_with("protocol_error:"));
    assert!(!client.is_connected());
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[test]
fn scoped_search_valid_incomplete_result_and_interior_carriage_return_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(capability_peer(
        directory.path(),
        scoped_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    let mut result = scoped_result();
    result["matches"][0]["text"] = serde_json::json!("needle\rinside");
    result["incomplete"] = serde_json::json!(true);
    result["reasons"] = serde_json::json!(["read_byte_limit"]);
    result["read_bytes"] = serde_json::json!(cedar_protocol::MAX_SCOPED_SEARCH_READ_BYTES);
    scoped_response(directory.path(), result);
    let Payload::ScopedMatches { result } = client.request(scoped_operation()).unwrap() else {
        panic!("wrong scoped-search response");
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
fn scoped_search_wrong_payload_disconnects_without_falling_back_to_legacy_search() {
    for payload in [
        serde_json::json!({"type": "matches", "matches": [], "truncated": false}),
        scoped_hello(serde_json::json!({"version": 1})),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(capability_peer(
            directory.path(),
            scoped_hello(serde_json::json!({"version": 1})),
        ))
        .unwrap();
        fs::write(
            directory.path().join("response-2.json"),
            serde_json::json!({"Ok": payload}).to_string(),
        )
        .unwrap();
        assert!(client
            .request(scoped_operation())
            .unwrap_err()
            .starts_with("protocol_error:"));
        assert!(!client.is_connected());
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
}

#[test]
fn scoped_search_late_hello_cannot_upgrade_and_reconnection_refreshes_support() {
    let old_root = tempfile::tempdir().unwrap();
    let mut old = capability_client(old_root.path(), Some(agent_info(&["list", "read"])));
    let snapshot = serde_json::to_value(old.handshake()).unwrap();
    fs::write(
        old_root.path().join("response-2.json"),
        serde_json::json!({"Ok": scoped_hello(serde_json::json!({"version": 1}))}).to_string(),
    )
    .unwrap();
    assert!(
        matches!(old.request(Operation::Read { path: "fixture.txt".into() }).unwrap(), Payload::Hello { scoped_search: Some(descriptor), .. } if descriptor.supported())
    );
    assert!(!old.supports_scoped_search());
    assert_eq!(serde_json::to_value(old.handshake()).unwrap(), snapshot);
    assert!(old
        .request(scoped_operation())
        .unwrap_err()
        .starts_with("unsupported_operation:"));
    old.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(old_root.path()).len(), 2);

    let fresh_root = tempfile::tempdir().unwrap();
    let mut fresh = Client::from_process(capability_peer(
        fresh_root.path(),
        scoped_hello(serde_json::json!({"version": 1})),
    ))
    .unwrap();
    assert!(fresh.supports_scoped_search());
    scoped_response(fresh_root.path(), scoped_result());
    assert!(matches!(
        fresh.request(scoped_operation()).unwrap(),
        Payload::ScopedMatches { .. }
    ));
    fresh.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(fresh_root.path()).len(), 2);

    let downgrade_root = tempfile::tempdir().unwrap();
    let mut downgrade =
        capability_client(downgrade_root.path(), Some(agent_info(&["list", "read"])));
    assert!(!downgrade.supports_scoped_search());
    assert!(downgrade
        .request(scoped_operation())
        .unwrap_err()
        .starts_with("unsupported_operation:"));
    downgrade.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(downgrade_root.path()).len(), 1);
}
