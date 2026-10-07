use cedar_protocol::{read_frame, write_frame, Operation, Payload, Request, Response};
use std::io::{BufReader, Read, Write};
use std::process::{Command, Stdio};

#[test]
fn stdio_agent_handles_sequential_frames_without_stdout_noise() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cedar-agent"))
        .arg("--root")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let ops = [
        Operation::Hello,
        Operation::Write {
            path: "new.txt".into(),
            text: "hello".into(),
            expected_revision: None,
        },
        Operation::Read {
            path: "new.txt".into(),
        },
        Operation::Run {
            program: "echo".into(),
            args: vec![],
            timeout_secs: 1,
        },
        Operation::Read {
            path: "../outside".into(),
        },
        Operation::List { path: "".into() },
        Operation::GitStatus,
    ];
    for (i, op) in ops.into_iter().enumerate() {
        write_frame(&mut input, &Request { id: i as u64, op }).unwrap();
        let response: Response = read_frame(&mut output).unwrap().unwrap();
        assert_eq!(response.id, i as u64);
        match i {
            0 => assert!(matches!(
                response.result,
                Ok(Payload::Hello { protocol, .. }) if protocol == cedar_protocol::PROTOCOL_VERSION
            )),
            1 => assert!(matches!(response.result, Ok(Payload::Written { .. }))),
            2 => assert!(
                matches!(response.result, Ok(Payload::File { ref text, .. }) if text == "hello")
            ),
            3 => assert_eq!(response.result.unwrap_err().code, "run_disabled"),
            4 => assert_eq!(response.result.unwrap_err().code, "invalid_path"),
            5 => assert!(matches!(response.result, Ok(Payload::Entries { .. }))),
            _ => assert_eq!(response.result.unwrap_err().code, "run_disabled"),
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    assert!(read_frame::<_, Response>(&mut output).unwrap().is_none());
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.is_empty());
}

#[test]
fn invalid_frames_fail_closed_and_diagnostics_use_stderr() {
    let dir = tempfile::tempdir().unwrap();
    for malformed in [b"not json\n".as_slice(), b"{\"id\":1}".as_slice()] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cedar-agent"))
            .arg("--root")
            .arg(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(malformed).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("protocol stream closed"));
    }
}

#[test]
fn invalid_arguments_and_help_never_emit_stdout() {
    for args in [vec![], vec!["--unknown"], vec!["--root"], vec!["--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_cedar-agent"))
            .args(&args)
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(output.status.success(), args == ["--help"]);
    }
}

#[test]
fn standard_binary_has_no_validation_cli_even_with_all_features() {
    let root = tempfile::tempdir().unwrap();
    for flag in ["--synthetic-root", "--windows-language-validation"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cedar-agent"))
            .arg("--root")
            .arg(root.path())
            .arg("--allow-run")
            .arg(flag)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown or duplicate argument"));
    }
}
