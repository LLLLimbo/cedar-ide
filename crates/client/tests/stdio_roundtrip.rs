//! Run with CEDAR_AGENT_BIN=/absolute/path/to/cedar-agent cargo test -p cedar-client --test stdio_roundtrip -- --ignored.
use cedar_client::Client;
use cedar_protocol::{Operation, Payload};
use std::path::PathBuf;
#[test]
#[ignore = "requires a compiled cedar-agent; scripts/verify.sh runs this explicitly"]
fn process_round_trip_preserves_revisions() {
    let agent = PathBuf::from(std::env::var_os("CEDAR_AGENT_BIN").expect("set CEDAR_AGENT_BIN"));
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::spawn_agent(&agent, dir.path(), false).unwrap();
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        panic!("current real agent must advertise metadata");
    };
    info.validate().unwrap();
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.os, std::env::consts::OS);
    assert_eq!(info.arch, std::env::consts::ARCH);
    let supports_run = info.supports("run");
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
        .starts_with(if supports_run {
            "run_disabled:"
        } else {
            "unsupported_operation:"
        }));
    drop(client);
    let mut reconnect = Client::spawn_agent(&agent, dir.path(), false).unwrap();
    assert!(
        matches!(reconnect.request(Operation::Read{path:"Hello.java".into()}).unwrap(),Payload::File{text,..} if text=="class Changed {}\n")
    );
}

// These Linux tests use the real agent's owned process groups. They do not
// simulate SSH or claim that SIGKILL/network loss can clean up a remote task.
#[cfg(target_os = "linux")]
mod lifecycle {
    use super::*;
    use cedar_protocol::{read_frame, write_frame, Request, Response};
    use std::{
        fs,
        io::{BufReader, Read, Write},
        path::Path,
        process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    fn agent() -> PathBuf {
        std::env::var_os("CEDAR_AGENT_BIN")
            .expect("set CEDAR_AGENT_BIN")
            .into()
    }
    fn eventually(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !predicate() {
            assert!(
                Instant::now() < deadline,
                "lifecycle condition did not complete within 4s"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn live_command() -> Operation {
        Operation::RunStart {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "sleep 20 & child=$!; printf '%s %s\n' \"$$\" \"$child\" > task-pids; wait".into(),
            ],
            timeout_secs: 30,
        }
    }
    fn task_id(payload: Payload) -> u64 {
        let Payload::RunTask { snapshot } = payload else {
            panic!("expected task snapshot")
        };
        snapshot["id"].as_u64().unwrap()
    }
    fn pids(root: &Path) -> Vec<u32> {
        let path = root.join("task-pids");
        eventually(|| fs::read_to_string(&path).is_ok_and(|s| s.split_whitespace().count() == 2));
        fs::read_to_string(path)
            .unwrap()
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect()
    }
    fn running(pid: u32) -> bool {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let state = stat.rsplit_once(") ").unwrap().1.chars().next().unwrap();
        !matches!(state, 'Z' | 'X')
    }
    fn assert_task_stopped(pids: &[u32]) {
        eventually(|| {
            pids.iter().all(|pid| !running(*pid))
                && !Path::new(&format!("/proc/{}", pids[0])).exists()
        });
        // The TaskManager reaps its leader; orphan descendant zombies, if any,
        // are the init process's responsibility, not a reaping guarantee here.
        assert!(
            !Path::new(&format!("/proc/{}", pids[0])).exists(),
            "task leader was not reaped"
        );
    }

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn dropping_client_closes_agent_input_and_stops_owned_task_group() {
        let root = tempfile::tempdir().unwrap();
        let mut client = Client::spawn_agent(&agent(), root.path(), true).unwrap();
        let _task = task_id(client.request(live_command()).unwrap());
        let pids = pids(root.path());
        assert!(pids.iter().all(|pid| running(*pid)));
        let start = Instant::now();
        drop(client);
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "drop waited on the caller thread"
        );
        assert_task_stopped(&pids);
    }

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn active_task_allows_file_work_cancel_and_reconnect_without_adoption() {
        let root = tempfile::tempdir().unwrap();
        let mut client = Client::spawn_agent(&agent(), root.path(), true).unwrap();
        let old_id = task_id(client.request(live_command()).unwrap());
        let pids = pids(root.path());
        client
            .request(Operation::Write {
                path: "during.txt".into(),
                text: "while running 中文".into(),
                expected_revision: None,
            })
            .unwrap();
        assert!(
            matches!(client.request(Operation::Read { path: "during.txt".into() }).unwrap(), Payload::File { text, .. } if text == "while running 中文")
        );
        assert!(client
            .request(live_command())
            .unwrap_err()
            .starts_with("task_busy:"));
        client
            .request(Operation::RunCancel { task_id: old_id })
            .unwrap();
        eventually(|| {
            let Payload::RunTask { snapshot } = client
                .request(Operation::RunPoll { task_id: old_id })
                .unwrap()
            else {
                panic!("task payload")
            };
            snapshot["state"] == "cancelled"
        });
        assert_task_stopped(&pids);
        drop(client);
        let mut fresh = Client::spawn_agent(&agent(), root.path(), true).unwrap();
        for op in [
            Operation::RunPoll { task_id: old_id },
            Operation::RunCancel { task_id: old_id },
        ] {
            assert!(fresh.request(op).unwrap_err().starts_with("unknown_task:"));
        }
        let new_id = task_id(
            fresh
                .request(Operation::RunStart {
                    program: "sh".into(),
                    args: vec!["-c".into(), "printf fresh-session".into()],
                    timeout_secs: 3,
                })
                .unwrap(),
        );
        // The numeric value may be reused in a new agent. UI generation + task
        // ID, not task ID alone, identifies a task across reconnection.
        eventually(|| {
            let Payload::RunTask { snapshot } = fresh
                .request(Operation::RunPoll { task_id: new_id })
                .unwrap()
            else {
                panic!("task payload")
            };
            if snapshot["state"] != "succeeded" {
                return false;
            }
            assert_eq!(snapshot["stdout"], "fresh-session");
            true
        });
        assert!(fresh.is_connected());
    }

    struct RawAgent {
        child: Child,
        input: Option<ChildStdin>,
        output: Option<BufReader<ChildStdout>>,
        next_id: u64,
    }
    impl RawAgent {
        fn new(root: &Path) -> Self {
            let mut child = Command::new(agent())
                .arg("--root")
                .arg(root)
                .arg("--allow-run")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let input = child.stdin.take();
            let output = child.stdout.take().map(BufReader::new);
            Self {
                child,
                input,
                output,
                next_id: 0,
            }
        }
        fn request(&mut self, op: Operation) -> Payload {
            self.next_id += 1;
            write_frame(
                self.input.as_mut().unwrap(),
                &Request {
                    id: self.next_id,
                    op,
                },
            )
            .unwrap();
            let mut output = self.output.take().unwrap();
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let response = read_frame::<_, Response>(&mut output);
                let _ = tx.send((output, response));
            });
            let (output, response) = rx
                .recv_timeout(Duration::from_secs(3))
                .expect("agent response exceeded 3s");
            self.output = Some(output);
            let response = response.unwrap().unwrap();
            assert_eq!(response.id, self.next_id);
            response.result.unwrap()
        }
        fn exit(&mut self) -> ExitStatus {
            let mut status = None;
            eventually(|| {
                status = self.child.try_wait().unwrap();
                status.is_some()
            });
            status.unwrap()
        }
    }
    impl Drop for RawAgent {
        fn drop(&mut self) {
            self.input.take();
            self.output.take();
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn forced_agent_termination_does_not_imply_owned_task_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let mut agent = RawAgent::new(root.path());
        task_id(agent.request(Operation::RunStart {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "sleep 2 & child=$!; printf '%s %s\n' \"$$\" \"$child\" > task-pids; wait".into(),
            ],
            timeout_secs: 10,
        }));
        let pids = pids(root.path());
        agent.child.kill().unwrap();
        assert!(!agent.exit().success());
        // No Workspace destructor runs after SIGKILL. This fixture terminates
        // naturally after two seconds, so observing the gap leaves no live task.
        assert!(
            pids.iter().all(|pid| running(*pid)),
            "fixture must expose the forced-kill cleanup gap"
        );
        eventually(|| pids.iter().all(|pid| !running(*pid)));
    }

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn real_agent_eof_broken_pipe_and_malformed_input_unwind_active_tasks() {
        for failure in ["eof", "broken_pipe", "truncated", "oversized"] {
            let root = tempfile::tempdir().unwrap();
            let mut agent = RawAgent::new(root.path());
            task_id(agent.request(live_command()));
            let pids = pids(root.path());
            match failure {
                "eof" => {
                    agent.input.take();
                }
                "broken_pipe" => {
                    // The response pipe is gone, but stdin stays open so the
                    // failure is specifically write_frame -> BrokenPipe.
                    agent.output.take();
                    write_frame(
                        agent.input.as_mut().unwrap(),
                        &Request {
                            id: 99,
                            op: Operation::Hello,
                        },
                    )
                    .unwrap();
                }
                "truncated" => {
                    agent
                        .input
                        .as_mut()
                        .unwrap()
                        .write_all(b"{\"id\":99")
                        .unwrap();
                    agent.input.take();
                }
                "oversized" => {
                    // Agent may close input before write_all finishes; either
                    // write result is valid as long as it exits with an error.
                    let mut input = agent.input.take().unwrap();
                    let (sent, result) = mpsc::channel();
                    thread::spawn(move || {
                        let _ = input.write_all(&vec![b'x'; cedar_protocol::MAX_FRAME_BYTES + 1]);
                        drop(input);
                        let _ = sent.send(());
                    });
                    result
                        .recv_timeout(Duration::from_secs(3))
                        .expect("oversized input send exceeded 3s");
                }
                _ => unreachable!(),
            }
            let status = agent.exit();
            assert_eq!(status.success(), failure == "eof", "{failure}: {status}");
            assert_task_stopped(&pids);
            let mut diagnostics = String::new();
            agent
                .child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut diagnostics)
                .unwrap();
            if failure != "eof" {
                assert!(
                    diagnostics.contains("protocol stream closed:"),
                    "{failure}: {diagnostics}"
                );
            }
            if failure == "broken_pipe" {
                assert!(
                    diagnostics.to_ascii_lowercase().contains("broken pipe"),
                    "{diagnostics}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires installed OpenSSH; verify/CI runs this config-only test explicitly"]
fn openssh_effective_config_cannot_relax_localhost_host_keys_or_stdio() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("isolated-ssh-config");
    // This test only invokes ssh -G with our file. No authentication, server,
    // key read/generation, network connection, or user configuration is involved.
    std::fs::write(&config, "Host *\n  BatchMode no\n  StrictHostKeyChecking no\n  NoHostAuthenticationForLocalhost yes\n  UpdateHostKeys yes\n  CheckHostIP yes\n  AddKeysToAgent yes\n  ForwardAgent yes\n  ForwardX11 yes\n  ClearAllForwardings no\n  PermitLocalCommand yes\n  ControlMaster auto\n  StdinNull yes\n  SessionType none\n  ForkAfterAuthentication yes\n").unwrap();
    let output = std::process::Command::new("ssh")
        .args(["-G", "-F"])
        .arg(config)
        .args(
            cedar_client::ssh_arguments("localhost", 22, "/fixture", "cedar-agent", false).unwrap(),
        )
        .output()
        .expect("OpenSSH required for config-only transport validation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let options: std::collections::HashMap<_, _> = text
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect();
    for key in ["batchmode", "stricthostkeychecking", "clearallforwardings"] {
        assert!(
            matches!(options.get(key), Some(&"yes" | &"true")),
            "{key}: {text}"
        );
    }
    for key in [
        "nohostauthenticationforlocalhost",
        "updatehostkeys",
        "checkhostip",
        "addkeystoagent",
        "forwardagent",
        "forwardx11",
        "permitlocalcommand",
        "controlmaster",
    ] {
        assert!(
            matches!(options.get(key), Some(&"no" | &"false")),
            "{key}: {text}"
        );
    }
    // These aliases did not exist before OpenSSH 8.7; if supported, the
    // command-line override must defeat the conflicting test config.
    for (key, expected) in [
        ("stdinnull", "no"),
        ("sessiontype", "default"),
        ("forkafterauthentication", "no"),
    ] {
        if let Some(value) = options.get(key) {
            assert_eq!(*value, expected, "{key}: {text}");
        }
    }
}
