//! Opt-in real-adapter test. The Linux-only containment guard is intentionally
//! separate from production: direct adapter cleanup cannot guarantee descendants.
#![cfg(target_os = "linux")]
use cedar_debugger::{DapClient, Event, Options, ProcessConfig};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

// Test process becomes subreaper so owned orphaned descendants can be reaped.
// This is not enabled by the production transport.
unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}
#[derive(Clone, Debug)]
struct Identity {
    pid: u32,
    start_time: String,
    state: String,
    parent: u32,
}
fn processes() -> HashMap<u32, Identity> {
    let mut result = HashMap::new();
    for entry in fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some((_, rest)) = stat.rsplit_once(") ") else {
            continue;
        };
        let fields: Vec<_> = rest.split_whitespace().collect();
        if fields.len() < 20 {
            continue;
        }
        result.insert(
            pid,
            Identity {
                pid,
                start_time: fields[19].into(),
                state: fields[0].into(),
                parent: fields[1].parse().unwrap(),
            },
        );
    }
    result
}
fn descendants(root: u32, all: &HashMap<u32, Identity>) -> Vec<Identity> {
    let mut pids = HashSet::from([root]);
    loop {
        let prior = pids.len();
        for p in all.values() {
            if pids.contains(&p.parent) {
                pids.insert(p.pid);
            }
        }
        if prior == pids.len() {
            break;
        }
    }
    pids.remove(&root);
    pids.into_iter()
        .filter_map(|pid| all.get(&pid).cloned())
        .collect()
}
fn alive(identity: &Identity) -> bool {
    processes()
        .get(&identity.pid)
        .is_some_and(|p| p.start_time == identity.start_time && p.state != "Z")
}
struct Containment;
impl Drop for Containment {
    fn drop(&mut self) {
        for identity in descendants(std::process::id(), &processes()) {
            if alive(&identity) {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &identity.pid.to_string()])
                    .status();
            }
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            let mut status = 0;
            let pid = unsafe { waitpid(-1, &mut status, 1) };
            if pid == -1 {
                return;
            }
            if pid == 0 {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
fn validate_socket_event(body: &Value) -> usize {
    let sockets = body["sockets"].as_array().expect("debugpySockets array");
    for socket in sockets {
        assert_eq!(
            socket["host"], "127.0.0.1",
            "non-loopback debugpy endpoint: {socket}"
        );
        // debugpy 1.8.22 also opens a loopback client listener in stdio
        // mode and labels it internal:false. Do not misrepresent this as
        // exclusively internal or socket-free operation.
        assert!(socket["internal"].is_boolean());
    }
    sockets.len()
}
fn await_event(client: &DapClient, name: &str, socket_events: &mut usize) -> Value {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < until,
            "waiting for {name}; stderr/output {:?}",
            client.output_tail()
        );
        match client.next_event(Duration::from_millis(100)) {
            Some(Event::Adapter { event, body, .. }) => {
                let body = body.unwrap_or(Value::Null);
                if event == "debugpySockets" {
                    *socket_events += validate_socket_event(&body);
                }
                if event == name {
                    return body;
                }
            }
            Some(other) => panic!("unexpected {other:?}; output {:?}", client.output_tail()),
            None => {}
        }
    }
}
fn inspect_live_listeners(identities: &[Identity]) -> usize {
    let mut inodes = HashSet::new();
    for identity in identities {
        if let Ok(entries) = fs::read_dir(format!("/proc/{}/fd", identity.pid)) {
            for entry in entries.flatten() {
                if let Ok(link) = fs::read_link(entry.path()) {
                    let link = link.to_string_lossy();
                    if let Some(inode) = link
                        .strip_prefix("socket:[")
                        .and_then(|s| s.strip_suffix(']'))
                    {
                        inodes.insert(inode.to_owned());
                    }
                }
            }
        }
    }
    let mut listeners = 0;
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        for line in fs::read_to_string(table).unwrap().lines().skip(1) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 10 || fields[3] != "0A" || !inodes.contains(fields[9]) {
                continue;
            }
            let addr = fields[1].split(':').next().unwrap();
            assert!(
                addr == "0100007F" || addr == "00000000000000000000000001000000",
                "non-loopback owned listener: {line}"
            );
            listeners += 1;
        }
    }
    listeners
}
fn stop_while_paused(python: &Path, fixture: &Path, mode: &str) {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("fixture.pid");
    let mut config = ProcessConfig::new(python);
    config.args = ["-X", "frozen_modules=off", "-m", "debugpy.adapter"]
        .into_iter()
        .map(Into::into)
        .collect();
    let mut client = DapClient::spawn(
        config,
        Options {
            shutdown_timeout: Duration::from_secs(5),
            ..Options::default()
        },
    )
    .unwrap();
    client.initialize("debugpy").unwrap().wait().unwrap();
    let launch = client
        .request(
            "launch",
            Some(json!({
                "name":"Cedar synthetic fixture","type":"python","request":"launch",
                "program":fixture,"python":[python],"args":[marker],
                "console":"internalConsole","redirectOutput":true,"subProcess":false,
                "debugAdapterHost":"127.0.0.1","justMyCode":true,"stopOnEntry":false
            })),
        )
        .unwrap();
    let mut socket_events = 0;
    await_event(&client, "initialized", &mut socket_events);
    let line = fs::read_to_string(fixture)
        .unwrap()
        .lines()
        .position(|line| line.contains("# CEDAR_BREAKPOINT"))
        .unwrap()
        + 1;
    let breakpoints = client
        .request(
            "setBreakpoints",
            Some(json!({"source":{"path":fixture},"breakpoints":[{"line":line}]})),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        breakpoints.body.unwrap()["breakpoints"][0]["verified"],
        true
    );
    client
        .request("configurationDone", Some(json!({})))
        .unwrap()
        .wait()
        .unwrap();
    launch.wait().unwrap();
    let stopped = await_event(&client, "stopped", &mut socket_events);
    assert_eq!(stopped["reason"], "breakpoint");
    let thread_id = stopped["threadId"].as_u64().unwrap();
    let threads = client
        .request("threads", None)
        .unwrap()
        .wait()
        .unwrap()
        .body
        .unwrap();
    assert!(threads["threads"]
        .as_array()
        .unwrap()
        .iter()
        .any(|thread| thread["id"] == thread_id));
    let stack = client
        .request("stackTrace", Some(json!({"threadId":thread_id})))
        .unwrap()
        .wait()
        .unwrap()
        .body
        .unwrap();
    let frame = &stack["stackFrames"][0];
    assert_eq!(frame["line"], line);
    assert_eq!(frame["source"]["path"], fixture.to_string_lossy().as_ref());
    let scopes = client
        .request("scopes", Some(json!({"frameId":frame["id"]})))
        .unwrap()
        .wait()
        .unwrap()
        .body
        .unwrap();
    let locals = scopes["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scope| scope["name"] == "Locals")
        .unwrap();
    let vars = client
        .request(
            "variables",
            Some(json!({"variablesReference":locals["variablesReference"]})),
        )
        .unwrap()
        .wait()
        .unwrap()
        .body
        .unwrap();
    assert!(vars["variables"]
        .as_array()
        .unwrap()
        .iter()
        .any(|var| var["name"] == "answer" && var["value"] == "41"));
    let all = processes();
    let adapter = all.get(&client.process_id()).unwrap().clone();
    let mut owned = descendants(adapter.pid, &all);
    owned.push(adapter.clone());
    let fixture_pid: u32 = fs::read_to_string(marker).unwrap().parse().unwrap();
    assert!(owned.iter().any(|p| p.pid == fixture_pid));
    assert!(socket_events > 0, "expected explicit debugpy socket events");
    let live_listeners = inspect_live_listeners(&owned);
    assert!(live_listeners > 0, "expected observed internal listener");
    println!("{mode}: real breakpoint line {line}; stack and answer=41 verified; {live_listeners} live owned loopback listener(s), including its extra client endpoint");
    match mode {
        "resume" => {
            client
                .request("continue", Some(json!({"threadId":thread_id})))
                .unwrap()
                .wait()
                .unwrap();
            await_event(&client, "terminated", &mut socket_events);
            assert!(client.output_tail().text.contains("CEDAR_ANSWER=42"));
            client.disconnect(true).unwrap();
        }
        "graceful" => {
            client.disconnect(true).unwrap();
        }
        "crash" => {
            assert!(Command::new("/bin/kill")
                .args(["-KILL", &adapter.pid.to_string()])
                .status()
                .unwrap()
                .success());
            let start = Instant::now();
            while client.terminal_error().is_none() && start.elapsed() < Duration::from_secs(3) {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(client.terminal_error().is_some());
            client.stop();
        }
        "drop" => drop(client),
        _ => unreachable!(),
    }
    let start = Instant::now();
    while owned.iter().any(alive) && start.elapsed() < Duration::from_secs(3) {
        thread::sleep(Duration::from_millis(20));
    }
    let surviving: Vec<_> = owned.iter().filter(|p| alive(p)).map(|p| p.pid).collect();
    println!("{mode}: surviving owned processes after transport cleanup: {surviving:?}");
    assert!(!alive(&adapter), "direct adapter must be gone");
    if mode == "resume" || mode == "graceful" {
        assert!(surviving.is_empty(), "graceful cleanup failed");
    }
    // Kill only identities proven to belong to this synthetic test, with PID
    // reuse checking. Production deliberately makes no descendant guarantee.
    for p in &owned {
        if alive(p) {
            assert!(Command::new("/bin/kill")
                .args(["-KILL", &p.pid.to_string()])
                .status()
                .unwrap()
                .success());
        }
    }
    let end = Instant::now() + Duration::from_secs(3);
    while owned.iter().any(alive) && Instant::now() < end {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !owned.iter().any(alive),
        "test cleanup left a running process"
    );
    // Adopted descendants are safely reaped by the test-only subreaper.
    for p in owned {
        let mut status = 0;
        let _ = unsafe { waitpid(p.pid as i32, &mut status, 1) };
    }
}
#[test]
#[ignore = "requires explicit CEDAR_DEBUGPY_PYTHON pointing to an isolated debugpy 1.8.22 installation"]
fn real_python_breakpoints_scopes_resume_and_cleanup_modes() {
    assert_eq!(
        unsafe { prctl(36, 1, 0, 0, 0) },
        0,
        "enable Linux test-only child subreaper"
    );
    let _containment = Containment;
    let python =
        PathBuf::from(std::env::var_os("CEDAR_DEBUGPY_PYTHON").expect("set CEDAR_DEBUGPY_PYTHON"));
    let output = Command::new(&python)
        .args(["-c", "import debugpy; print(debugpy.__version__)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "1.8.22");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/breakpoint.py");
    for mode in ["resume", "graceful", "crash", "drop"] {
        stop_while_paused(&python, &fixture, mode);
    }
}
