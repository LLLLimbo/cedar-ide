//! Feature-gated test driver. Copy this executable beside the exact agent under
//! test: `Client::connect(Local)` resolves a sibling of this process, not cargo.
#[cfg(windows)]
fn main() {
    use cedar_client::{Client, ConnectionSpec};
    use cedar_protocol::{Operation, Payload, PROTOCOL_VERSION, RUN_TASK_CAPABILITIES};
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    // Bound even a regression in the production client's 30s request deadline.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(12));
        eprintln!("bundle probe watchdog expired");
        std::process::exit(126);
    });
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mode = args[0].to_str().unwrap();
    let root = PathBuf::from(&args[1]);
    let connect = |allow_run| {
        Client::connect(ConnectionSpec::Local {
            root: root.clone(),
            allow_run,
        })
    };
    if mode == "missing" || mode == "invalid" {
        let error = match connect(true) {
            Ok(_) => panic!("{mode}: Local unexpectedly connected using a fallback"),
            Err(error) => error,
        };
        assert!(
            error.starts_with(if mode == "missing" {
                "bundled_agent_missing:"
            } else {
                "bundled_agent_start_failed:"
            }),
            "{mode}: {error}"
        );
        assert!(
            error.contains("cedar-agent"),
            "{mode}: actionable agent error: {error}"
        );
        println!("expected-error:{error}");
        return;
    }
    assert_eq!(mode, "success");
    let fixture = args[2].to_str().unwrap().to_owned();
    let mut untrusted = connect(false).unwrap();
    let Payload::Hello {
        protocol,
        root: reported,
        agent: Some(info),
    } = untrusted.handshake()
    else {
        panic!("local Windows agent omitted metadata");
    };
    assert_eq!(*protocol, PROTOCOL_VERSION);
    assert_eq!(*reported, root.canonicalize().unwrap().to_string_lossy());
    info.validate().unwrap();
    assert_eq!(info.os, "windows");
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    for cap in ["list", "read", "write", "search"]
        .iter()
        .chain(RUN_TASK_CAPABILITIES)
    {
        assert!(info.supports(cap), "missing {cap}");
    }
    for cap in ["run", "git_status", "language_start", "terminal"] {
        assert!(!info.supports(cap), "unexpected {cap}");
    }
    let saved_info = info.clone();
    assert!(untrusted
        .request(Operation::Read {
            path: "absent.txt".into()
        })
        .is_err());
    let task = || Operation::RunStart {
        program: fixture.clone(),
        args: vec!["exit".into(), "0".into()],
        timeout_secs: 3,
    };
    assert!(untrusted
        .request(task())
        .unwrap_err()
        .starts_with("run_disabled:"));
    assert!(untrusted
        .request(Operation::Run {
            program: fixture.clone(),
            args: vec![],
            timeout_secs: 1,
        })
        .unwrap_err()
        .starts_with("unsupported_operation:"));
    assert!(untrusted.is_connected());
    let Payload::Hello {
        agent: Some(after), ..
    } = untrusted.request(Operation::Hello).unwrap()
    else {
        panic!("hello payload");
    };
    assert_eq!(after, saved_info);
    drop(untrusted);

    let mut trusted = connect(true).unwrap();
    let Payload::RunTask { snapshot } = trusted.request(task()).unwrap() else {
        panic!("task payload")
    };
    let id = snapshot["id"].as_u64().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let Payload::RunTask { snapshot } =
            trusted.request(Operation::RunPoll { task_id: id }).unwrap()
        else {
            panic!("poll payload")
        };
        if !matches!(
            snapshot["state"].as_str(),
            Some("starting" | "running" | "cancelling")
        ) {
            assert_eq!(snapshot["state"], "succeeded", "{snapshot}");
            assert_eq!(snapshot["windows_exit_code"], 0);
            assert_eq!(snapshot["exit_code"], 0);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Local task routing timed out: {snapshot}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(trusted.is_connected());
    println!("bundle-ok:metadata,trust,file-errors,async-tasks");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("cedar-client-bundle-probe requires real Windows");
    std::process::exit(1);
}
