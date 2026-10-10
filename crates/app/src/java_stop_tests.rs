use super::*;
use serde_json::json;

fn linux() -> Value {
    json!({"stopped":true,"shutdown":{
        "platform":"linux","status":"graceful","reason":"root_exited",
        "root_exit":{"kind":"code","code":0},"cleanup_joined":true,
        "shutdown_response_received":true,"exit_frame_completed":true
    }})
}

fn windows() -> Value {
    json!({"stopped":true,"shutdown":{
        "status":"graceful","reason":"root_exited","root_exit_code":0,
        "cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true
    }})
}

#[test]
fn exact_legacy_windows_body_preserves_native_u32_exit_codes() {
    let mut value = windows();
    assert_eq!(value["shutdown"].as_object().unwrap().len(), 6);
    assert_eq!(
        JavaStopOutcome::parse(&value).unwrap().root_exit,
        JavaRootExit::WindowsCode(0)
    );
    for code in [259, 1067, u32::MAX] {
        value["shutdown"]["status"] = json!("error");
        value["shutdown"]["reason"] = json!("transport_failure");
        value["shutdown"]["root_exit_code"] = json!(code);
        let outcome = JavaStopOutcome::parse(&value).unwrap();
        assert_eq!(outcome.root_exit, JavaRootExit::WindowsCode(code));
        assert_eq!(outcome.root_exit_code(), Some(code));
        assert!(outcome.message().contains(&format!("exit {code}")));
    }
    for code in [json!(-1), json!(4_294_967_296u64), json!(0.0), json!("0")] {
        value["shutdown"]["root_exit_code"] = code;
        assert!(JavaStopOutcome::parse(&value).is_err());
    }
}

#[test]
fn linux_exit_codes_and_signals_remain_distinct_and_bounded() {
    let mut value = linux();
    assert_eq!(value["shutdown"].as_object().unwrap().len(), 7);
    assert_eq!(
        JavaStopOutcome::parse(&value).unwrap().message(),
        "Java server exited gracefully (exit 0)."
    );
    for status in ["forced", "error"] {
        value["shutdown"]["status"] = json!(status);
        value["shutdown"]["shutdown_response_received"] = json!(false);
        value["shutdown"]["exit_frame_completed"] = json!(false);
        value["private"] = json!("private server stderr");
        for reason in [
            "root_exited",
            "grace_expired",
            "aborted",
            "transport_failure",
            "worker_panicked",
        ] {
            value["shutdown"]["reason"] = json!(reason);
            for code in [0, 255] {
                value["shutdown"]["root_exit"] = json!({"kind":"code","code":code});
                let outcome = JavaStopOutcome::parse(&value).unwrap();
                assert_eq!(outcome.root_exit, JavaRootExit::LinuxCode(code));
                assert_eq!(outcome.root_exit_code(), Some(u32::from(code)));
                assert!(outcome.message().contains(&format!("exit {code}")));
                assert!(!outcome.message().contains("gracefully"));
            }
            for signal in [1, 9, 64] {
                value["shutdown"]["root_exit"] = json!({"kind":"signal","signal":signal});
                let outcome = JavaStopOutcome::parse(&value).unwrap();
                assert_eq!(outcome.root_exit, JavaRootExit::LinuxSignal(signal));
                assert_eq!(outcome.root_exit_code(), None);
                let message = outcome.message();
                assert!(message.contains(&format!("signal {signal}")));
                assert!(!message.contains("exit 137"));
                assert!(!message.contains("gracefully"));
                assert!(!message.contains("private"));
                assert!(message.len() < 160);
            }
        }
    }
}

#[test]
fn linux_graceful_requires_zero_code_and_all_protocol_witnesses() {
    for (field, invalid) in [
        ("root_exit", json!({"kind":"code","code":1})),
        ("root_exit", json!({"kind":"signal","signal":9})),
        ("reason", json!("grace_expired")),
        ("cleanup_joined", json!(false)),
        ("shutdown_response_received", json!(false)),
        ("exit_frame_completed", json!(false)),
    ] {
        let mut value = linux();
        value["shutdown"][field] = invalid;
        assert!(JavaStopOutcome::parse(&value).is_err(), "{field}: {value}");
    }
}

#[test]
fn stop_body_rejects_unknown_mixed_missing_and_wrongly_typed_fields() {
    for good in [windows(), linux()] {
        for field in good["shutdown"].as_object().unwrap().keys() {
            let mut missing = good.clone();
            missing["shutdown"].as_object_mut().unwrap().remove(field);
            assert!(JavaStopOutcome::parse(&missing).is_err(), "missing {field}");
            let mut null = good.clone();
            null["shutdown"][field] = Value::Null;
            assert!(JavaStopOutcome::parse(&null).is_err(), "null {field}");
        }
        for (field, invalid) in [
            ("status", json!("unknown")),
            ("status", json!({"graceful":null})),
            ("reason", json!("unknown")),
            ("reason", json!({"root_exited":null})),
            ("cleanup_joined", json!(false)),
            ("cleanup_joined", json!(1)),
            ("shutdown_response_received", json!("true")),
            ("exit_frame_completed", json!(1)),
            ("unknown", json!("private payload")),
        ] {
            let mut value = good.clone();
            value["shutdown"][field] = invalid;
            assert!(JavaStopOutcome::parse(&value).is_err(), "{field}: {value}");
        }
        for stopped in [Value::Null, json!(false), json!("true"), json!(1)] {
            let mut value = good.clone();
            value["stopped"] = stopped;
            assert!(JavaStopOutcome::parse(&value).is_err());
        }
    }
    for platform in [json!("linux"), json!("windows"), Value::Null] {
        let mut value = windows();
        value["shutdown"]["platform"] = platform;
        assert!(JavaStopOutcome::parse(&value).is_err());
    }
    let mut mixed = linux();
    mixed["shutdown"]["root_exit_code"] = json!(0);
    assert!(JavaStopOutcome::parse(&mixed).is_err());
    let mut mixed = windows();
    mixed["shutdown"]["root_exit"] = json!({"kind":"code","code":0});
    assert!(JavaStopOutcome::parse(&mixed).is_err());
    for platform in [
        json!("windows"),
        json!("Linux"),
        json!("macos"),
        json!(1),
        json!({"linux":null}),
    ] {
        let mut value = linux();
        value["shutdown"]["platform"] = platform;
        assert!(JavaStopOutcome::parse(&value).is_err());
    }
}

#[test]
fn linux_root_exit_rejects_unknown_mixed_missing_and_out_of_range_values() {
    for root_exit in [
        Value::Null,
        json!(0),
        json!({}),
        json!({"kind":"unknown","code":0}),
        json!({"kind":"code"}),
        json!({"kind":"signal"}),
        json!({"kind":"code","code":-1}),
        json!({"kind":"code","code":256}),
        json!({"kind":"code","code":0.0}),
        json!({"kind":"code","code":"0"}),
        json!({"kind":"signal","signal":0}),
        json!({"kind":"signal","signal":65}),
        json!({"kind":"signal","signal":-1}),
        json!({"kind":"signal","signal":9.0}),
        json!({"kind":"signal","signal":"9"}),
        json!({"kind":"code","signal":9}),
        json!({"kind":"signal","code":0}),
        json!({"kind":"code","code":0,"signal":9}),
        json!({"kind":"signal","signal":9,"code":137}),
        json!({"kind":"code","code":0,"unknown":true}),
    ] {
        let mut value = linux();
        value["shutdown"]["status"] = json!("forced");
        value["shutdown"]["root_exit"] = root_exit;
        assert!(JavaStopOutcome::parse(&value).is_err(), "{value}");
    }
}

#[test]
fn typed_stop_deserialization_rejects_duplicate_body_and_exit_fields() {
    for value in [windows(), linux()] {
        let encoded = serde_json::to_string(&value["shutdown"]).unwrap();
        assert!(serde_json::from_str::<JavaStopOutcome>(&encoded).is_ok());
        let duplicate = encoded.replacen(
            "\"status\":\"graceful\"",
            "\"status\":\"graceful\",\"status\":\"graceful\"",
            1,
        );
        assert!(serde_json::from_str::<JavaStopOutcome>(&duplicate).is_err());
    }
    let encoded = serde_json::to_string(&linux()["shutdown"]).unwrap();
    for (field, duplicated) in [
        ("\"code\":0", "\"code\":0,\"code\":0"),
        ("\"kind\":\"code\"", "\"kind\":\"code\",\"kind\":\"code\""),
        (
            "\"platform\":\"linux\"",
            "\"platform\":\"linux\",\"platform\":\"linux\"",
        ),
    ] {
        let duplicate = encoded.replacen(field, duplicated, 1);
        assert!(serde_json::from_str::<JavaStopOutcome>(&duplicate).is_err());
    }
}
