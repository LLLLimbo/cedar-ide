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
        io::{self, BufReader, PipeWriter, Read, Write},
        os::fd::{AsFd, AsRawFd, OwnedFd},
        path::Path,
        process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
        sync::{mpsc, Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    fn agent() -> PathBuf {
        std::env::var_os("CEDAR_AGENT_BIN")
            .expect("set CEDAR_AGENT_BIN")
            .into()
    }
    fn eventually(context: &str, mut predicate: impl FnMut() -> Result<(), String>) {
        let deadline = Instant::now() + Duration::from_secs(4);
        eventually_before(context, deadline, &mut predicate);
    }
    fn eventually_before(
        context: &str,
        deadline: Instant,
        mut predicate: impl FnMut() -> Result<(), String>,
    ) {
        while let Err(detail) = predicate() {
            assert!(
                Instant::now() < deadline,
                "{context} did not complete within 4s: {detail}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn pipe_has_no_readers(writer: &PipeWriter) -> io::Result<bool> {
        let mut descriptor = libc::pollfd {
            fd: writer.as_raw_fd(),
            events: 0,
            revents: 0,
        };
        // Linux reports POLLERR on a pipe's write end only once every read
        // descriptor is closed. This passive check cannot fill/block the pipe.
        // https://man7.org/linux/man-pages/man2/poll.2.html
        // SAFETY: descriptor is live, initialized, and valid for this one-entry
        // poll. The writer remains owned for the entire nonblocking call.
        if unsafe { libc::poll(&mut descriptor, 1, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if descriptor.revents & libc::POLLNVAL != 0 {
            return Err(io::Error::other("stdout probe descriptor is invalid"));
        }
        Ok(descriptor.revents & libc::POLLERR != 0)
    }
    fn wait_for_no_pipe_readers(writer: &PipeWriter, deadline: Instant) {
        eventually_before("broken_pipe: all stdout readers closed", deadline, || {
            if pipe_has_no_readers(writer).map_err(|error| error.to_string())? {
                Ok(())
            } else {
                Err("stdout still has a retained read descriptor".into())
            }
        });
    }
    fn live_command() -> Operation {
        Operation::RunStart {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "sleep 20 & child=$!; printf '%s %s\n' \"$$\" \"$child\" > task-pids.tmp && mv task-pids.tmp task-pids; wait".into(),
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
    fn pids(root: &Path, context: &str) -> Vec<u32> {
        let path = root.join("task-pids");
        let mut ready = None;
        eventually(&format!("{context}: PID fixture readiness"), || {
            let snapshot = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let parsed = snapshot
                .split_whitespace()
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("invalid PID snapshot {snapshot:?}: {error}"))?;
            // The fixture publishes by rename. Also require its final newline
            // and retain this exact read, never a second unchecked snapshot:
            // two tokens alone can accept a partially written second PID.
            if !snapshot.ends_with('\n') || parsed.len() != 2 || parsed.contains(&0) {
                return Err(format!("incomplete PID snapshot {snapshot:?}"));
            }
            ready = Some(parsed);
            Ok(())
        });
        ready.unwrap()
    }
    fn running(pid: u32) -> bool {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let state = stat.rsplit_once(") ").unwrap().1.chars().next().unwrap();
        !matches!(state, 'Z' | 'X')
    }
    fn process_diagnostics(pids: &[u32]) -> String {
        pids.iter()
            .map(|pid| {
                format!(
                    "pid {pid}: stat={:?}; wchan={:?}",
                    fs::read_to_string(format!("/proc/{pid}/stat")),
                    fs::read_to_string(format!("/proc/{pid}/wchan"))
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
    fn assert_task_stopped(pids: &[u32], context: &str) {
        eventually(
            &format!("{context}: task group stopped and leader reaped"),
            || {
                if pids.iter().all(|pid| !running(*pid))
                    && !Path::new(&format!("/proc/{}", pids[0])).exists()
                {
                    Ok(())
                } else {
                    Err(format!(
                        "running={:?}; leader present={}; {}",
                        pids.iter().filter(|pid| running(**pid)).collect::<Vec<_>>(),
                        Path::new(&format!("/proc/{}", pids[0])).exists(),
                        process_diagnostics(pids)
                    ))
                }
            },
        );
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
        let pids = pids(root.path(), "client drop");
        assert!(pids.iter().all(|pid| running(*pid)));
        let start = Instant::now();
        drop(client);
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "drop waited on the caller thread"
        );
        assert_task_stopped(&pids, "client drop");
    }

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn active_task_allows_file_work_cancel_and_reconnect_without_adoption() {
        let root = tempfile::tempdir().unwrap();
        let mut client = Client::spawn_agent(&agent(), root.path(), true).unwrap();
        let old_id = task_id(client.request(live_command()).unwrap());
        let pids = pids(root.path(), "cancel and reconnect");
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
        eventually("cancel and reconnect: cancelled snapshot", || {
            let Payload::RunTask { snapshot } = client
                .request(Operation::RunPoll { task_id: old_id })
                .unwrap()
            else {
                panic!("task payload")
            };
            if snapshot["state"] == "cancelled" {
                Ok(())
            } else {
                Err(snapshot.to_string())
            }
        });
        assert_task_stopped(&pids, "cancel and reconnect");
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
        eventually("cancel and reconnect: fresh task succeeded", || {
            let Payload::RunTask { snapshot } = fresh
                .request(Operation::RunPoll { task_id: new_id })
                .unwrap()
            else {
                panic!("task payload")
            };
            if snapshot["state"] != "succeeded" {
                return Err(snapshot.to_string());
            }
            assert_eq!(snapshot["stdout"], "fresh-session");
            Ok(())
        });
        assert!(fresh.is_connected());
    }

    struct RawAgent {
        child: Child,
        input: Option<ChildStdin>,
        output: Option<BufReader<ChildStdout>>,
        stderr: Arc<Mutex<Vec<u8>>>,
        stderr_reader: Option<thread::JoinHandle<()>>,
        next_id: u64,
    }
    impl RawAgent {
        fn new(root: &Path) -> Self {
            Self::with_stdout(root, Stdio::piped())
        }
        fn with_stdout_probe(root: &Path) -> (Self, PipeWriter) {
            let (reader, writer) = io::pipe().unwrap();
            let mut agent = Self::with_stdout(root, writer.try_clone().unwrap().into());
            agent.output = Some(BufReader::new(ChildStdout::from(OwnedFd::from(reader))));
            (agent, writer)
        }
        fn with_stdout(root: &Path, stdout: Stdio) -> Self {
            let mut child = Command::new(agent())
                .arg("--root")
                .arg(root)
                .arg("--allow-run")
                .stdin(Stdio::piped())
                .stdout(stdout)
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let input = child.stdin.take();
            let output = child.stdout.take().map(BufReader::new);
            // Drain while the agent is alive so failure diagnostics cannot
            // fill its pipe and become the reason it cannot exit.
            let mut pipe = child.stderr.take().unwrap();
            let stderr = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&stderr);
            let stderr_reader = thread::spawn(move || {
                let mut buffer = [0; 1024];
                loop {
                    match pipe.read(&mut buffer) {
                        Ok(0) => return,
                        Ok(count) => {
                            let mut bytes = captured.lock().unwrap();
                            let keep = count.min(16 * 1024 - bytes.len());
                            bytes.extend_from_slice(&buffer[..keep]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => panic!("agent stderr capture failed: {error}"),
                    }
                }
            });
            Self {
                child,
                input,
                output,
                stderr,
                stderr_reader: Some(stderr_reader),
                next_id: 0,
            }
        }
        fn diagnostics(&self) -> String {
            String::from_utf8_lossy(&self.stderr.lock().unwrap()).into_owned()
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
        fn exit(&mut self, context: &str, task_pids: &[u32]) -> ExitStatus {
            self.exit_before(context, task_pids, Instant::now() + Duration::from_secs(4))
        }
        fn exit_before(
            &mut self,
            context: &str,
            task_pids: &[u32],
            deadline: Instant,
        ) -> ExitStatus {
            let mut status = None;
            eventually_before(
                &format!("{context}: agent exit and stderr EOF"),
                deadline,
                || {
                    status = self.child.try_wait().unwrap();
                    let stderr_finished = self.stderr_reader.as_ref().unwrap().is_finished();
                    if status.is_some() && stderr_finished {
                        Ok(())
                    } else {
                        Err(format!(
                        "exit={status:?}; stderr EOF={stderr_finished}; agent: {}; task: {}; stderr={:?}",
                        process_diagnostics(&[self.child.id()]),
                        process_diagnostics(task_pids),
                        self.diagnostics()
                    ))
                    }
                },
            );
            self.stderr_reader.take().unwrap().join().unwrap();
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
                "sleep 2 & child=$!; printf '%s %s\n' \"$$\" \"$child\" > task-pids.tmp && mv task-pids.tmp task-pids; wait".into(),
            ],
            timeout_secs: 10,
        }));
        let pids = pids(root.path(), "forced termination");
        agent.child.kill().unwrap();
        assert!(!agent.exit("forced termination", &pids).success());
        // No Workspace destructor runs after SIGKILL. This fixture terminates
        // naturally after two seconds, so observing the gap leaves no live task.
        assert!(
            pids.iter().all(|pid| running(*pid)),
            "fixture must expose the forced-kill cleanup gap"
        );
        eventually("forced termination: fixture exited naturally", || {
            if pids.iter().all(|pid| !running(*pid)) {
                Ok(())
            } else {
                Err(process_diagnostics(&pids))
            }
        });
    }

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn real_agent_eof_broken_pipe_and_malformed_input_unwind_active_tasks() {
        for failure in ["eof", "broken_pipe", "truncated", "oversized"] {
            let root = tempfile::tempdir().unwrap();
            let (mut agent, stdout_probe) = if failure == "broken_pipe" {
                let (agent, probe) = RawAgent::with_stdout_probe(root.path());
                (agent, Some(probe))
            } else {
                (RawAgent::new(root.path()), None)
            };
            task_id(agent.request(live_command()));
            let pids = pids(root.path(), failure);
            assert!(
                pids.iter().all(|pid| running(*pid)),
                "{failure}: task fixture must be live before fault injection: {}",
                process_diagnostics(&pids)
            );
            let mut broken_pipe_deadline = None;
            match failure {
                "eof" => {
                    agent.input.take();
                }
                "broken_pipe" => {
                    let deadline = Instant::now() + Duration::from_secs(4);
                    broken_pipe_deadline = Some(deadline);
                    agent.output.take();
                    // A parallel fork can retain a CLOEXEC read descriptor
                    // until exec. Prove every reader is gone before sending
                    // the one response that must fail; merely dropping our
                    // reader can let that response succeed and strand the
                    // agent waiting on still-open stdin. Readiness and exit
                    // share the original four-second bound.
                    wait_for_no_pipe_readers(stdout_probe.as_ref().unwrap(), deadline);
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
            let status = if let Some(deadline) = broken_pipe_deadline {
                agent.exit_before(failure, &pids, deadline)
            } else {
                agent.exit(failure, &pids)
            };
            assert_eq!(status.success(), failure == "eof", "{failure}: {status}");
            assert_task_stopped(&pids, failure);
            let diagnostics = agent.diagnostics();
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

    #[test]
    #[ignore = "requires compiled cedar-agent; scripts/verify.sh and CI run this explicitly"]
    fn broken_pipe_probe_accounts_for_a_retained_stdout_reader() {
        let root = tempfile::tempdir().unwrap();
        let (mut agent, stdout_probe) = RawAgent::with_stdout_probe(root.path());
        task_id(agent.request(live_command()));
        let pids = pids(root.path(), "retained stdout reader");
        assert!(pids.iter().all(|pid| running(*pid)));
        let duplicate = agent
            .output
            .as_ref()
            .unwrap()
            .get_ref()
            .as_fd()
            .try_clone_to_owned()
            .unwrap();
        agent.output.take();
        assert!(
            !pipe_has_no_readers(&stdout_probe).unwrap(),
            "closing one reader must not establish a broken response pipe"
        );
        // Deterministically model the descriptor retained by a concurrent
        // fork: the first response succeeds even after our usual reader closes.
        // request() bounds the read and verifies its response ID.
        agent.output = Some(BufReader::new(ChildStdout::from(duplicate)));
        assert!(matches!(
            agent.request(Operation::Hello),
            Payload::Hello { .. }
        ));
        assert!(agent.child.try_wait().unwrap().is_none());
        assert!(pids.iter().all(|pid| running(*pid)));

        let deadline = Instant::now() + Duration::from_secs(4);
        agent.output.take();
        wait_for_no_pipe_readers(&stdout_probe, deadline);
        write_frame(
            agent.input.as_mut().unwrap(),
            &Request {
                id: 99,
                op: Operation::Hello,
            },
        )
        .unwrap();
        assert!(!agent
            .exit_before("retained stdout reader", &pids, deadline)
            .success());
        assert_task_stopped(&pids, "retained stdout reader");
        let diagnostics = agent.diagnostics();
        assert!(
            diagnostics.contains("protocol stream closed:"),
            "{diagnostics}"
        );
        assert!(
            diagnostics.to_ascii_lowercase().contains("broken pipe"),
            "{diagnostics}"
        );
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
