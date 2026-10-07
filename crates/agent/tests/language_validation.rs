#![cfg(feature = "windows-language-validation")]
use cedar_protocol::{read_frame, write_frame, Operation, Request, Response};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn fixture_requires_marked_synthetic_root_and_explicit_execution_trust() {
    let root = tempfile::tempdir().unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_cedar-agent-language-validation"))
        .arg("--synthetic-root")
        .arg(root.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    std::fs::write(
        root.path().join(".cedar-windows-language-validation"),
        b"cedar-windows-language-validation-v1\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cedar-agent-language-validation"))
        .arg("--synthetic-root")
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut frames = Vec::new();
    for (id, op) in [
        Operation::LanguageStart {
            program: "must-not-run".into(),
            args: vec![],
        },
        Operation::RunStart {
            program: "must-not-run".into(),
            args: vec![],
            timeout_secs: 1,
        },
        Operation::LanguageStop,
        Operation::RunCancel { task_id: 1 },
    ]
    .into_iter()
    .enumerate()
    {
        write_frame(&mut frames, &Request { id: id as u64, op }).unwrap();
    }
    child.stdin.take().unwrap().write_all(&frames).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut responses = output.stdout.as_slice();
    for id in 0..4 {
        let response: Response = read_frame(&mut responses).unwrap().unwrap();
        assert_eq!(response.id, id);
        assert_eq!(response.result.unwrap_err().code, "run_disabled");
    }
    assert!(read_frame::<_, Response>(&mut responses).unwrap().is_none());
    assert!(output.stderr.is_empty());
}
