//! Standalone std-only fault peer, compiled by transport_tests on every platform.
//! It is never part of the shipped binaries and never opens a network socket.
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn marker(directory: &Path, name: &str, contents: &[u8]) {
    let pending = directory.join(format!("{name}.pending"));
    fs::write(&pending, contents).unwrap();
    fs::rename(pending, directory.join(name)).unwrap();
}

fn drain_until_close(input: &mut impl Read, directory: &Path) {
    let mut rest = Vec::new();
    input.read_to_end(&mut rest).unwrap();
    OpenOptions::new()
        .append(true)
        .open(directory.join("requests"))
        .unwrap()
        .write_all(&rest)
        .unwrap();
    marker(directory, "eof", b"orderly input close");
}

fn wait_for_release(directory: &Path, id: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !directory.join(format!("release-{id}")).is_file() {
        if Instant::now() >= deadline {
            marker(
                directory,
                "safety-expired",
                b"controller did not release reply",
            );
            std::process::exit(91);
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn emit(bytes: &[u8]) {
    let mut stdout = io::stdout().lock();
    if stdout
        .write_all(bytes)
        .and_then(|_| stdout.flush())
        .is_err()
    {
        std::process::exit(0);
    }
}
fn hello(id: u64, protocol: u32) {
    emit(format!("{{\"id\":{id},\"result\":{{\"Ok\":{{\"type\":\"hello\",\"protocol\":{protocol},\"root\":\"/fixture\"}}}}}}\n").as_bytes());
}
// New capability peers have a caller-supplied first Hello and deterministic,
// side-effect-free responses. Every request is recorded before it is answered.
fn capability_reply(id: u64, request: &str, directory: &Path) {
    if let Ok(result) = fs::read_to_string(directory.join(format!("response-{id}.json"))) {
        emit(format!("{{\"id\":{id},\"result\":{result}}}\n").as_bytes());
        return;
    }
    let payload = if request.contains("\"type\":\"hello\"") {
        // A second wire Hello is deliberately inconsistent. The public Client
        // must return its original snapshot without ever transmitting this.
        "{\"type\":\"hello\",\"protocol\":4,\"root\":\"/changed-after-connect\"}"
    } else if request.contains("\"type\":\"list\"") {
        "{\"type\":\"entries\",\"entries\":[]}"
    } else if request.contains("\"type\":\"read\"") {
        "{\"type\":\"file\",\"path\":\"fixture.txt\",\"text\":\"fixture text\",\"revision\":\"fixture-revision\"}"
    } else if request.contains("\"type\":\"write\"") {
        "{\"type\":\"written\",\"revision\":\"written-revision\"}"
    } else if request.contains("\"type\":\"search\"") {
        "{\"type\":\"matches\",\"matches\":[],\"truncated\":false}"
    } else if request.contains("\"type\":\"git_status\"") {
        "{\"type\":\"git_status\",\"text\":\"\"}"
    } else if request.contains("\"type\":\"run\"") {
        "{\"type\":\"run\",\"stdout\":\"\",\"stderr\":\"\",\"exit_code\":0,\"timed_out\":false,\"truncated\":false}"
    } else if request.contains("\"type\":\"run_") {
        "{\"type\":\"run_task\",\"snapshot\":{}}"
    } else if request.contains("\"type\":\"language_start\"")
        || request.contains("\"type\":\"language_start_java\"")
    {
        "{\"type\":\"language\",\"value\":{\"started\":true}}"
    } else {
        "{\"type\":\"language\",\"value\":null}"
    };
    emit(format!("{{\"id\":{id},\"result\":{{\"Ok\":{payload}}}}}\n").as_bytes());
}

// Only the opt-in fixture binary exposes this relay. The normal, unmodified
// agent still owns its workspace and receives precisely `--root <fixture>`.
// Polling below is test control, never part of the shipping client transport.
const RELAY_MAX_BYTES: usize = 32 * 1024 * 1024;
const RELAY_MAX_FRAME: usize = 8 * 1024 * 1024;
const RELAY_MAX_REQUESTS: usize = 64;
const RELAY_WATCHDOG: Duration = Duration::from_secs(30);

fn fixture_error(message: &str) -> io::Error {
    io::Error::other(message)
}

struct RelayChild {
    child: Child,
    wait_owned: bool,
}

impl Drop for RelayChild {
    fn drop(&mut self) {
        if self.wait_owned {
            let _ = self.child.kill();
            loop {
                match self.child.wait() {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    _ => break,
                }
            }
        }
    }
}

enum RelayInput {
    Frame(Vec<u8>),
    Closed,
    Failed,
}

fn normal_agent_relay(directory: &Path, disconnect_on_owner_eof: bool) -> io::Result<()> {
    let mut configured = Vec::new();
    fs::File::open(directory.join("relay-agent-path"))?
        .take(4097)
        .read_to_end(&mut configured)?;
    if configured.len() > 4096 || configured.iter().any(|b| matches!(b, b'\r' | b'\n' | 0)) {
        return Err(fixture_error("invalid bounded normal-agent path"));
    }
    let agent = PathBuf::from(
        String::from_utf8(configured).map_err(|_| fixture_error("agent path must be UTF-8"))?,
    );
    if !agent.is_absolute() || !agent.is_file() {
        return Err(fixture_error("normal-agent path must be an absolute file"));
    }
    let mut owner = RelayChild {
        child: Command::new(agent)
            .arg("--root")
            .arg(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
        wait_owned: true,
    };
    let process_id = owner.child.id();
    let mut agent_input = owner.child.stdin.take();
    let mut agent_output = owner
        .child
        .stdout
        .take()
        .ok_or_else(|| fixture_error("missing agent stdout"))?;
    marker(
        directory,
        "relay-agent-started",
        process_id.to_string().as_bytes(),
    );

    let (input_tx, input_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(io::stdin());
        loop {
            let mut line = Vec::new();
            let received = (&mut reader)
                .take((RELAY_MAX_FRAME + 1) as u64)
                .read_until(b'\n', &mut line);
            let message = match received {
                Ok(0) => RelayInput::Closed,
                Ok(_) if line.len() <= RELAY_MAX_FRAME && line.last() == Some(&b'\n') => {
                    RelayInput::Frame(line)
                }
                _ => RelayInput::Failed,
            };
            let terminal = !matches!(&message, RelayInput::Frame(_));
            if input_tx.send(message).is_err() || terminal {
                break;
            }
        }
    });
    let (output_tx, output_rx) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            let mut forwarded = 0usize;
            let mut buffer = [0u8; 8192];
            let mut output = io::stdout().lock();
            loop {
                let read = agent_output.read(&mut buffer)?;
                if read == 0 {
                    return Ok(());
                }
                forwarded += read;
                if forwarded > RELAY_MAX_BYTES {
                    return Err(fixture_error("relay response cap exceeded"));
                }
                output.write_all(&buffer[..read])?;
                output.flush()?;
            }
        })();
        let _ = output_tx.send(result);
    });

    let mut audit = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("requests"))?;
    let mut forwarded = 0usize;
    let mut requests = 0usize;
    let mut controlled_close = false;
    let mut owner_closed = false;
    let mut exit_success = None;
    let mut output_drained = false;
    let deadline = Instant::now() + RELAY_WATCHDOG;
    loop {
        if Instant::now() >= deadline {
            return Err(fixture_error("normal-agent relay watchdog expired"));
        }
        if !disconnect_on_owner_eof
            && !controlled_close
            && directory.join("relay-disconnect").exists()
        {
            let mut control = Vec::new();
            fs::File::open(directory.join("relay-disconnect"))?
                .take(32)
                .read_to_end(&mut control)?;
            if control != b"close-agent-stdin\n" {
                return Err(fixture_error("invalid idle-disconnect control"));
            }
            agent_input.take();
            controlled_close = true;
        }
        if !output_drained {
            match output_rx.try_recv() {
                Ok(result) => {
                    result?;
                    output_drained = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(fixture_error("relay response reader stopped"))
                }
            }
        }
        if exit_success.is_none() {
            match owner.child.try_wait() {
                Ok(Some(status)) => {
                    owner.wait_owned = false;
                    exit_success = Some(status.success());
                }
                Ok(None) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    // Do not signal a process after exclusive wait ownership
                    // becomes uncertain, even in this nonshipping fixture.
                    owner.wait_owned = false;
                    return Err(error);
                }
            }
        }
        if output_drained && exit_success.is_some() {
            if !controlled_close || exit_success != Some(true) {
                return Err(fixture_error(
                    "normal agent exited without successful controlled close",
                ));
            }
            marker(
                directory,
                "relay-agent-reaped",
                format!(
                "{{\"cleanup_verified\":true,\"process_id\":{process_id},\"exit_success\":true}}\n"
            )
                .as_bytes(),
            );
            return Ok(());
        }
        if owner_closed {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        match input_rx.recv_timeout(Duration::from_millis(5)) {
            Ok(RelayInput::Frame(line)) => {
                forwarded += line.len();
                requests += 1;
                if controlled_close || forwarded > RELAY_MAX_BYTES || requests > RELAY_MAX_REQUESTS
                {
                    return Err(fixture_error(
                        "relay request after close or request cap exceeded",
                    ));
                }
                audit.write_all(&line)?;
                audit.flush()?;
                let input = agent_input
                    .as_mut()
                    .ok_or_else(|| fixture_error("agent input is closed"))?;
                input.write_all(&line)?;
                input.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(RelayInput::Closed) if disconnect_on_owner_eof => {
                // This separately allowlisted mode exercises the frontend's
                // explicit local close. Keep the old idle-loss relay strict:
                // its owner EOF is still an error, never a passing receipt.
                agent_input.take();
                controlled_close = true;
                owner_closed = true;
                marker(directory, "relay-owner-eof", b"true\n");
            }
            Ok(RelayInput::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(fixture_error(
                    "relay owner closed before controlled completion",
                ));
            }
            Ok(RelayInput::Failed) => {
                return Err(fixture_error("invalid bounded relay input frame"))
            }
        }
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let mut mode = args.next().unwrap().to_string_lossy().into_owned();
    let dir = PathBuf::from(args.next().unwrap());
    if mode == "--root" {
        // Public Client::spawn_agent fixture route. It cannot be used as a
        // general workspace host and never accepts execution authorization.
        if args.next().is_some()
            || !fs::read(dir.join(".cedar-transport-fixture"))
                .is_ok_and(|contents| contents == b"cedar-transport-fixture-v1\n")
        {
            eprintln!("synthetic transport root required; --allow-run is forbidden");
            std::process::exit(2);
        }
        mode = fs::read_to_string(dir.join("fixture-mode")).unwrap_or_default();
        if !matches!(
            mode.as_str(),
            "stalled_hello"
                | "stalled_read"
                | "held_reply"
                | "held_reply_eof"
                | "capability_peer"
                | "idle_eof"
                | "idle_bad_frame"
                | "idle_unsolicited"
                | "normal_agent_idle_relay"
                | "normal_agent_disconnect_relay"
                | "disconnect_close_timeout"
        ) {
            eprintln!("unsupported synthetic transport mode");
            std::process::exit(2);
        }
    }
    fs::write(dir.join("started"), std::process::id().to_string()).unwrap();
    if matches!(
        mode.as_str(),
        "normal_agent_idle_relay" | "normal_agent_disconnect_relay"
    ) {
        if let Err(error) = normal_agent_relay(&dir, mode == "normal_agent_disconnect_relay") {
            eprintln!("normal-agent relay: {error}");
            std::process::exit(2);
        }
        return;
    }
    if mode == "eof_before_hello" {
        return;
    }
    if mode == "response_flood" {
        for _ in 0..100_000 {
            hello(1, 4);
        }
        return;
    }
    if mode == "disconnect_close_timeout" {
        let watchdog_directory = dir.clone();
        thread::spawn(move || {
            thread::sleep(RELAY_WATCHDOG);
            marker(
                &watchdog_directory,
                "safety-expired",
                b"close timeout fixture watchdog\n",
            );
            std::process::exit(91);
        });
    }
    let mut input = io::stdin().lock();
    let mut line = String::new();
    let mut count = 0;
    loop {
        line.clear();
        let read = if mode == "disconnect_close_timeout" {
            (&mut input)
                .take((RELAY_MAX_FRAME + 1) as u64)
                .read_line(&mut line)
        } else {
            input.read_line(&mut line)
        };
        if read.unwrap_or(0) == 0 {
            fs::write(dir.join("eof"), b"orderly input close").unwrap();
            return;
        }
        if mode == "disconnect_close_timeout"
            && (line.len() > RELAY_MAX_FRAME || !line.ends_with('\n'))
        {
            std::process::exit(2);
        }
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(dir.join("requests"))
            .unwrap()
            .write_all(line.as_bytes())
            .unwrap();
        count += 1;
        if mode == "disconnect_close_timeout" && count == 1 && !line.contains("\"type\":\"hello\"")
        {
            std::process::exit(2);
        }
        let id: u64 = line
            .split("\"id\":")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        if count == 1 {
            match mode.as_str() {
                "stalled_hello" => {
                    marker(&dir, &format!("ready-{id}"), line.as_bytes());
                    drain_until_close(&mut input, &dir);
                    return;
                }
                "capability_peer" => {
                    let payload = fs::read_to_string(dir.join("hello.json")).unwrap();
                    emit(format!("{{\"id\":{id},\"result\":{{\"Ok\":{payload}}}}}\n").as_bytes());
                    continue;
                }
                "old_hello" => {
                    hello(id, 3);
                    continue;
                }
                "new_hello" => {
                    hello(id, 5);
                    continue;
                }
                "malformed_hello" => {
                    emit(b"{\"id\":1,\"result\":{\"Ok\":{\"type\":\"hello\",\"protocol\":\"4\",\"root\":\"/\"}}}\n");
                    continue;
                }
                "missing_hello_field" => {
                    emit(b"{\"id\":1,\"result\":{\"Ok\":{\"type\":\"hello\",\"protocol\":4}}}\n");
                    continue;
                }
                "wrong_hello_payload" => {
                    emit(b"{\"id\":1,\"result\":{\"Ok\":{\"type\":\"entries\",\"entries\":[]}}}\n");
                    continue;
                }
                "wrong_id" => {
                    hello(id + 1, 4);
                    continue;
                }
                "truncated" => {
                    emit(b"{\"id\":1,\"result\":");
                    return;
                }
                "oversized" => {
                    emit(&vec![b'x'; 8 * 1024 * 1024 + 1]);
                    return;
                }
                "bad_json" => {
                    emit(b"not a JSON frame\n");
                    continue;
                }
                "stderr_flood" => {
                    let mut err = io::stderr().lock();
                    err.write_all(b"discarded-prefix").unwrap();
                    for _ in 0..4096 {
                        err.write_all(&[b'x'; 1024]).unwrap();
                    }
                    err.write_all(b"diagnostic-tail-marker\n").unwrap();
                    err.flush().unwrap();
                }
                "blocked_writer" => {
                    hello(id, 4);
                    thread::sleep(Duration::from_secs(10));
                    return;
                }
                _ => {}
            }
            if let Ok(payload) = fs::read_to_string(dir.join("hello.json")) {
                emit(format!("{{\"id\":{id},\"result\":{{\"Ok\":{payload}}}}}\n").as_bytes());
            } else {
                hello(id, 4);
            }
            if mode == "eof_between_requests" {
                return;
            }
            if matches!(
                mode.as_str(),
                "idle_eof" | "idle_bad_frame" | "idle_unsolicited"
            ) {
                marker(&dir, &format!("ready-{id}"), b"idle after Hello");
                wait_for_release(&dir, id);
                match mode.as_str() {
                    "idle_eof" => return,
                    "idle_bad_frame" => emit(b"invalid idle frame\n"),
                    "idle_unsolicited" => capability_reply(id + 1, "\"type\":\"read\"", &dir),
                    _ => unreachable!(),
                }
                drain_until_close(&mut input, &dir);
                return;
            }
            continue;
        }
        match mode.as_str() {
            "disconnect_close_timeout" => {
                // Only Hello and the ordinary initial List are admitted. After
                // replying, deliberately retain stdin without reading EOF.
                // This bounded synthetic stall leaves the Client's unchanged
                // two-second owned-child reaper responsible for termination.
                if count != 2 || !line.contains("\"type\":\"list\"") {
                    std::process::exit(2);
                }
                capability_reply(id, &line, &dir);
                marker(&dir, "close-timeout-ready", b"true\n");
                thread::sleep(Duration::from_secs(10));
                return;
            }
            "stalled_read" if line.contains("\"type\":\"read\"") => {
                marker(&dir, &format!("ready-{id}"), line.as_bytes());
                drain_until_close(&mut input, &dir);
                return;
            }
            "held_reply" | "held_reply_eof" => {
                marker(&dir, &format!("ready-{id}"), line.as_bytes());
                wait_for_release(&dir, id);
                capability_reply(id, &line, &dir);
                marker(&dir, &format!("replied-{id}"), b"reply flushed");
                if mode == "held_reply_eof" {
                    return;
                }
            }
            "stalled_read" => capability_reply(id, &line, &dir),
            "capability_peer" => capability_reply(id, &line, &dir),
            "eof_after_request" => return,
            "wrong_later_id" => hello(id - 1, 4),
            "stderr_flood" => {
                emit(b"invalid after diagnostics\n");
            }
            "write_unknown" | "stalled" => {
                if mode == "write_unknown" {
                    // Model an applied mutation whose acknowledgement is lost.
                    fs::write(dir.join("committed"), b"one write applied").unwrap();
                }
                drain_until_close(&mut input, &dir);
                return;
            }
            "stubborn" => {
                thread::sleep(Duration::from_secs(10));
                return;
            }
            _ => hello(id, 4),
        }
    }
}
