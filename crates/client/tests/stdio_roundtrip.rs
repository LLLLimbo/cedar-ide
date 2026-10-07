//! Run with CEDAR_AGENT_BIN=/absolute/path/to/cedar-agent cargo test -p cedar-client --test stdio_roundtrip.
use cedar_client::Client;
use cedar_protocol::{Operation, Payload};
use std::path::PathBuf;
#[test]
#[ignore = "requires a compiled cedar-agent; scripts/verify.sh runs this explicitly"]
fn process_round_trip_preserves_revisions() {
    let agent = PathBuf::from(std::env::var_os("CEDAR_AGENT_BIN").expect("set CEDAR_AGENT_BIN"));
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::spawn_agent(&agent, dir.path(), false).unwrap();
    assert!(matches!(
        client.request(Operation::Hello).unwrap(),
        Payload::Hello { .. }
    ));
    let first = client
        .request(Operation::Write {
            path: "Hello.java".into(),
            text: "class Hello {}\n".into(),
            expected_revision: None,
        })
        .unwrap();
    let Payload::Written { revision } = first else {
        panic!("wrong payload")
    };
    let read = client
        .request(Operation::Read {
            path: "Hello.java".into(),
        })
        .unwrap();
    assert!(matches!(read,Payload::File{text,..} if text=="class Hello {}\n"));
    std::fs::write(dir.path().join("Hello.java"), "class Changed {}\n").unwrap();
    let err = client
        .request(Operation::Write {
            path: "Hello.java".into(),
            text: "stale".into(),
            expected_revision: Some(revision),
        })
        .unwrap_err();
    assert!(err.starts_with("conflict:"));
    assert!(client.is_connected());
    assert!(client
        .request(Operation::Read {
            path: "../outside".into()
        })
        .is_err());
    let found = client
        .request(Operation::Search {
            query: "Changed".into(),
            limit: 20,
        })
        .unwrap();
    assert!(matches!(found,Payload::Matches{matches,..} if matches.len()==1));
    assert!(client
        .request(Operation::Run {
            program: "echo".into(),
            args: vec![],
            timeout_secs: 1
        })
        .unwrap_err()
        .starts_with("run_disabled:"));
    drop(client);
    let mut reconnect = Client::spawn_agent(&agent, dir.path(), false).unwrap();
    assert!(
        matches!(reconnect.request(Operation::Read{path:"Hello.java".into()}).unwrap(),Payload::File{text,..} if text=="class Changed {}\n")
    );
}
