//! Deterministic protocol peer used only with the `test-server` feature.
use cedar_language::framing::{read_frame, write_frame, FrameLimits};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::{self, BufReader, Write};
use std::thread;
use std::time::Duration;

fn send(output: &mut impl Write, value: Value) {
    write_frame(
        output,
        &serde_json::to_vec(&value).unwrap(),
        FrameLimits::default(),
    )
    .unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("normal");
    if mode == "exit-waits-eof" {
        thread::spawn(|| {
            thread::sleep(Duration::from_secs(8));
            std::process::exit(124);
        });
    }
    #[cfg(windows)]
    let _agent_tree = (mode == "win-agent-tree").then(|| windows_fixture::agent_tree(&args));
    #[cfg(windows)]
    if mode.starts_with("win-") && mode != "win-agent-tree" {
        windows_fixture::run(&args);
        return;
    }
    let mut audit = args
        .get(2)
        .filter(|_| mode != "win-agent-tree")
        .map(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap()
        });
    let mut output = io::stdout().lock();
    if mode == "eof" {
        return;
    }
    if mode == "bad-frame" {
        output.write_all(b"Content-Length: -1\r\n\r\n").unwrap();
        output.flush().unwrap();
        return;
    }
    if mode == "invalid-json" {
        write_frame(&mut output, b"{", FrameLimits::default()).unwrap();
        return;
    }
    if mode == "blocked-stdin" {
        thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut input = BufReader::new(io::stdin().lock());
    let mut initialized = false;
    let mut shutdown = false;
    let mut reverse_first = None;
    let mut server_request_sent = false;
    loop {
        let bytes = match read_frame(&mut input, FrameLimits::default()) {
            Ok(Some(bytes)) => bytes,
            _ => return,
        };
        let message: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(audit) = &mut audit {
            writeln!(audit, "{message}").unwrap();
            audit.flush().unwrap();
        }
        let method = message["method"].as_str().unwrap_or("");
        if method.is_empty() {
            // The test makes progress only after the client rejects a server request.
            if server_request_sent && message["id"] == "server-request" {
                assert_eq!(message["error"]["code"], -32601);
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","method":"mock/requestRejected","params":{}}),
                );
            }
            continue;
        }
        let id = message.get("id").cloned();
        let result = match method {
            "initialize" => {
                assert!(!initialized);
                if mode == "initialize-never" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","method":"mock/initializePending","params":{}}),
                    );
                    continue;
                }
                if mode == "initialize-blocked-notification" {
                    // The mandatory method-not-found reply exceeds the pipe's
                    // capacity. Leave it ahead of initialized and stop reading
                    // stdin after returning a successful initialize response.
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":"x".repeat(2 * 1024 * 1024),"method":"workspace/applyEdit","params":{}}),
                    );
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{}}}),
                    );
                    thread::sleep(Duration::from_secs(8));
                    return;
                }
                if mode == "delayed-initialize" {
                    thread::sleep(Duration::from_millis(250));
                }
                let mut capabilities = if mode == "no-capabilities" {
                    json!({})
                } else {
                    json!({"textDocumentSync":{"openClose":true,"change":if mode == "incremental" {2} else {1}}, "completionProvider":{"resolveProvider":true}, "definitionProvider":true,"hoverProvider":true,"documentFormattingProvider":true,"referencesProvider":true,"documentSymbolProvider":true,"workspaceSymbolProvider":true})
                };
                for capability in [
                    "documentFormattingProvider",
                    "referencesProvider",
                    "documentSymbolProvider",
                    "workspaceSymbolProvider",
                ] {
                    match mode {
                        "navigation-no-provider" => {
                            capabilities.as_object_mut().unwrap().remove(capability);
                        }
                        "navigation-false-provider" => capabilities[capability] = json!(false),
                        "navigation-invalid-provider" => capabilities[capability] = json!("true"),
                        "navigation-object-provider" => {
                            capabilities[capability] = json!({"workDoneProgress":false})
                        }
                        _ => {}
                    }
                }
                if mode == "resolve-no-provider" {
                    capabilities["completionProvider"] = json!({});
                }
                if mode == "resolve-false-provider" {
                    capabilities["completionProvider"]["resolveProvider"] = json!(false);
                }
                if mode == "bad-encoding" {
                    capabilities["positionEncoding"] = json!("utf-8");
                }
                json!({"capabilities":capabilities,"serverInfo":{"name":"Cedar mock"}})
            }
            "initialized" => {
                initialized = true;
                if mode == "ready-blocked-stdin" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","method":"mock/readyBlocked","params":{}}),
                    );
                    thread::sleep(Duration::from_secs(8));
                    return;
                }
                continue;
            }
            "textDocument/didOpen" | "textDocument/didChange" => {
                assert!(initialized);
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":message["params"]["textDocument"]["uri"],"version":message["params"]["textDocument"]["version"],"diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"severity":2,"source":"mock","message":"mock diagnostic"}]}}),
                );
                continue;
            }
            "textDocument/didClose" | "$/cancelRequest" => continue,
            "textDocument/completion" => {
                assert!(initialized);
                json!({"isIncomplete":false,"items":[{"label":"hello","kind":6,"data":{"resultId":17,"opaque":["keep",42]},"extension":{"future":true},"command":{"command":"mock.mustNotExecute","title":"Do not execute","arguments":[17]},"textEdit":{"newText":"hello","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}}}}]})
            }
            "completionItem/resolve" => {
                assert!(initialized);
                if mode == "resolve-never" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","method":"mock/resolvePending","params":{}}),
                    );
                    continue;
                }
                if mode == "resolve-error" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"unknown completion item","data":{"reason":"expired"}}}),
                    );
                    continue;
                }
                if mode == "resolve-invalid" {
                    Value::Null
                } else {
                    let mut item = message["params"].clone();
                    item["documentation"] =
                        json!({"kind":"plaintext","value":"Resolved mock documentation"});
                    item["detail"] = json!("demo.Hello");
                    item["additionalTextEdits"] = json!([{"newText":"import demo.Hello;\n","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}]);
                    item
                }
            }
            "textDocument/definition" => {
                json!({"uri":message["params"]["textDocument"]["uri"],"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}})
            }
            "textDocument/hover" => json!({"contents":{"kind":"plaintext","value":"mock hover"}}),
            "workspace/symbol" => {
                assert!(initialized && !shutdown);
                assert_eq!(message["params"].as_object().unwrap().len(), 1);
                assert!(message["params"]["query"].is_string());
                if mode == "workspace-symbol-error" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"mock workspace symbol error"}}),
                    );
                    continue;
                }
                if mode == "workspace-symbol-custom" {
                    let path =
                        std::path::Path::new(args.get(2).unwrap()).with_extension("result.json");
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
                } else {
                    json!([{"name":"Hello","kind":5,"containerName":"demo","location":{"uri":"file:///mock/Hello.java","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":5}}}}])
                }
            }
            "textDocument/formatting"
            | "textDocument/references"
            | "textDocument/documentSymbol" => {
                assert!(initialized && !shutdown);
                if mode == "navigation-error" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"mock feature error","data":{"method":method}}}),
                    );
                    continue;
                }
                if mode == "navigation-null" {
                    Value::Null
                } else if mode == "navigation-empty" {
                    json!([])
                } else {
                    let range =
                        json!({"start":{"line":0,"character":0},"end":{"line":0,"character":1}});
                    match method {
                        "textDocument/formatting" => {
                            json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"// mock formatted\n"}])
                        }
                        "textDocument/references" => {
                            json!([{"uri":message["params"]["textDocument"]["uri"],"range":range}])
                        }
                        _ if mode == "symbols-flat" => {
                            json!([{"name":"Hello","kind":5,"containerName":"demo","location":{"uri":message["params"]["textDocument"]["uri"],"range":range}}])
                        }
                        _ => {
                            json!([{"name":"Hello","kind":5,"range":range,"selectionRange":range,"children":[{"name":"count","kind":8,"range":range,"selectionRange":range}]}])
                        }
                    }
                }
            }
            "mock/error" => {
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"mock invalid parameters","data":{"detail":1}}}),
                );
                continue;
            }
            "mock/never" => {
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","method":"mock/pending","params":{}}),
                );
                continue;
            }
            "mock/reverse" => {
                if let Some((first_id, first_params)) = reverse_first.take() {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":id,"result":message["params"]}),
                    );
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":first_id,"result":first_params}),
                    );
                } else {
                    reverse_first = Some((id, message["params"].clone()));
                }
                continue;
            }
            "mock/flood" => {
                for n in 0..1000 {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","method":"mock/event","params":{"n":n}}),
                    );
                }
                json!("flood finished")
            }
            "mock/requestClient" => {
                server_request_sent = true;
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","id":"server-request","method":"workspace/applyEdit","params":{}}),
                );
                json!(true)
            }
            "mock/invalidResponse" => {
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","id":id,"result":true,"error":{"code":-1,"message":"bad"}}),
                );
                continue;
            }
            "mock/exit" => return,
            "shutdown" => {
                shutdown = true;
                if mode == "shutdown-error" {
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"shutdown refused"}}),
                    );
                    continue;
                }
                Value::Null
            }
            "exit" => {
                assert!(shutdown);
                if mode == "exit-waits-eof" {
                    assert!(message.get("id").is_none());
                    assert!(read_frame(&mut input, FrameLimits::default())
                        .expect("stdin must end at a frame boundary after exit")
                        .is_none());
                    let audit = audit.as_mut().expect("EOF fixture requires an audit file");
                    writeln!(audit, "{}", json!({"fixture":"stdin-eof-after-exit"})).unwrap();
                    audit.flush().unwrap();
                }
                if mode == "ignore-exit" {
                    thread::sleep(Duration::from_secs(30));
                }
                return;
            }
            _ => message["params"].clone(),
        };
        if let Some(id) = id {
            send(
                &mut output,
                json!({"jsonrpc":"2.0","id":id,"result":result}),
            );
        }
    }
}

/// Native transport fixtures are separate from the portable mock's protocol
/// modes. They never inspect user files or invoke a shell. Each invocation,
/// including descendants, has its own eight-second process lifetime cap.
#[cfg(windows)]
mod windows_fixture {
    use super::*;
    use std::fs::{self, File};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::path::Path;
    use std::process::{self, Command, Stdio};
    use std::time::Instant;

    // The full portable mock protocol runs with Windows-owned descendants, so
    // agent acceptance reaches initialize/open/query/shutdown through the real
    // Workspace bridge rather than a second mock implementation of that bridge.
    #[allow(clippy::zombie_processes)]
    pub(super) fn agent_tree(args: &[String]) -> File {
        let dir = Path::new(args.get(2).expect("synthetic fixture directory"));
        let lifetime = hold_lifetime(dir, "root");
        let expired = dir.join("root.expired");
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(8));
            let _ = fs::write(expired, b"fixture lifetime cap reached");
            process::exit(124);
        });
        let _child = spawn_descendant(dir, "win-descendant");
        wait_file(&dir.join("child.ready"));
        fs::write(dir.join("root.ready"), b"ready").unwrap();
        if let Some(task_ready) = args.get(3) {
            // Paired with tree-coexist: neither synthetic startup can finish
            // until the other has started, without depending on sleeps.
            wait_file(Path::new(task_ready));
        }
        lifetime
    }

    // Descendants deliberately outlive their parents to test Job cleanup.
    // Windows has no Unix zombie-reaping requirement; the owner Job must kill them.
    #[allow(clippy::zombie_processes)]
    pub(super) fn run(args: &[String]) {
        let mode = args[1].as_str();
        let dir = Path::new(args.get(2).expect("fixture directory"));
        let lifetime_name = match mode {
            "win-descendant" | "win-descendant-stderr" => "child",
            "win-grandchild" | "win-grandchild-stderr" => "grandchild",
            _ => "root",
        };
        let watchdog_file = dir.join(format!("{lifetime_name}.expired"));
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(8));
            let _ = fs::write(watchdog_file, b"fixture lifetime cap reached");
            process::exit(124);
        });
        // Keep an exclusive, non-inheritable file open for this process's
        // entire lifetime. Tests observe this exact file, never a reusable PID.
        let _lifetime = hold_lifetime(dir, lifetime_name);
        match mode {
            "win-descendant" | "win-descendant-stderr" => {
                let grandchild = if mode.ends_with("-stderr") {
                    "win-grandchild-stderr"
                } else {
                    "win-grandchild"
                };
                let _child = spawn_descendant(dir, grandchild);
                wait_file(&dir.join("grandchild.ready"));
                fs::write(dir.join("child.ready"), b"ready").unwrap();
                idle();
            }
            "win-grandchild" | "win-grandchild-stderr" => {
                fs::write(dir.join("grandchild.ready"), b"ready").unwrap();
                idle();
            }
            "win-lsp-exit-needs-stdin-eof"
            | "win-lsp-graceful-eof-release"
            | "win-lsp-graceful-eof-stalled"
            | "win-lsp-graceful-eof-malformed" => {
                lsp_shutdown(mode, dir);
                return;
            }
            "win-tree-blocked" | "win-tree-exit" | "win-stdout-eof-tree" => {
                if mode == "win-stdout-eof-tree" {
                    exclude_original_stdout_from_inheritance();
                    fs::write(dir.join("stdout-noninheritable.ready"), b"verified").unwrap();
                }
                let descendant = if mode == "win-stdout-eof-tree" {
                    "win-descendant-stderr"
                } else {
                    "win-descendant"
                };
                let _child = spawn_descendant(dir, descendant);
                wait_file(&dir.join("child.ready"));
                ready();
                if mode == "win-tree-blocked" {
                    idle();
                }
                wait_file(&dir.join("go"));
                if mode == "win-tree-exit" {
                    // Descendants retain both pipes and do no more I/O. Only
                    // root-exit observation, not pipe EOF, can stop the owner.
                    return;
                }
                close_standard_handle(io::stdout().as_raw_handle());
                // Root and descendants retain stderr; stdout EOF must stop the
                // connection without waiting for any of them to exit naturally.
                idle();
            }
            "win-initialize-partial" => {
                use std::io::Read;
                ready();
                // Observe exactly one byte, then stop consuming. A large
                // initialize frame is now partly delivered and cannot replay.
                io::stdin().read_exact(&mut [0_u8; 1]).unwrap();
                send(
                    &mut io::stdout().lock(),
                    json!({"jsonrpc":"2.0","method":"mock/initializePartial","params":{}}),
                );
                idle();
            }
            "win-blocked-stdin" => {
                ready();
                idle();
            }
            "win-stall-header" | "win-trickle-header" | "win-stall-body" | "win-trickle-body" => {
                ready();
                wait_file(&dir.join("go"));
                let mut output = io::stdout().lock();
                let body = mode.ends_with("body");
                output
                    .write_all(if body {
                        b"Content-Length: 4096\r\n\r\n{"
                    } else {
                        b"C"
                    })
                    .unwrap();
                output.flush().unwrap();
                if mode.contains("trickle") {
                    // Every byte arrives inside the request timeout. A sliding
                    // inactivity timer would incorrectly keep this frame alive.
                    for byte in if body {
                        vec![b' '; 80]
                    } else {
                        b"ontent-Length: 4096\r\nX-Ignored: ".repeat(3)
                    } {
                        thread::sleep(Duration::from_millis(75));
                        if output.write_all(&[byte]).is_err() || output.flush().is_err() {
                            return;
                        }
                    }
                }
                idle();
            }
            "win-stderr-eof" => {
                close_standard_handle(io::stderr().as_raw_handle());
            }
            "win-echo" | "win-stderr-flood" | "win-final-response" => {}
            _ => panic!("unknown native fixture mode {mode}"),
        }
        ready();
        let mut input = BufReader::new(io::stdin().lock());
        while let Ok(Some(bytes)) = read_frame(&mut input, FrameLimits::default()) {
            let message: Value = serde_json::from_slice(&bytes).unwrap();
            match message["method"].as_str() {
                Some("mock/floodStderr") => {
                    assert_eq!(mode, "win-stderr-flood");
                    let mut stderr = io::stderr().lock();
                    stderr.write_all(b"discarded-stderr-prefix\n").unwrap();
                    for _ in 0..128 {
                        stderr.write_all(&[b'E'; 8192]).unwrap();
                    }
                    stderr.write_all(b"\nretained-stderr-suffix\n").unwrap();
                    stderr.flush().unwrap();
                }
                Some("mock/fail") => {
                    // The preceding flood response acknowledges that the full
                    // flood was drained before the test asks for this failure.
                    io::stdout()
                        .write_all(b"Content-Length: -1\r\n\r\n")
                        .unwrap();
                    io::stdout().flush().unwrap();
                    idle();
                }
                Some("mock/exit") => return,
                _ => {}
            }
            if let Some(id) = message.get("id") {
                send(
                    &mut io::stdout().lock(),
                    json!({"jsonrpc":"2.0","id":id,"result":message["params"]}),
                );
                if mode == "win-final-response" {
                    return;
                }
            }
        }
    }

    fn ready() {
        send(
            &mut io::stdout().lock(),
            json!({"jsonrpc":"2.0","method":"mock/ready","params":{}}),
        );
    }

    #[allow(clippy::zombie_processes)]
    fn lsp_shutdown(mode: &str, dir: &Path) {
        exclude_original_stdout_from_inheritance();
        let _child = spawn_descendant(dir, "win-descendant-stderr");
        wait_file(&dir.join("child.ready"));
        // Open the synchronization files before closing stdout. Later writes and
        // metadata reads use these held handles without recycling stdout's value.
        let mut closed = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("stdout-closed.ready"))
            .unwrap();
        let release = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(dir.join("release"))
            .unwrap();
        ready();
        let mut input = BufReader::new(io::stdin().lock());
        let mut initialize_replied = false;
        let mut initialized = false;
        let mut shutdown = false;
        loop {
            let bytes = read_frame(&mut input, FrameLimits::default())
                .unwrap()
                .expect("stdin closed before complete exit notification");
            let message: Value = serde_json::from_slice(&bytes).unwrap();
            let result = match message["method"].as_str().unwrap() {
                "initialize" => {
                    assert!(!initialize_replied);
                    initialize_replied = true;
                    json!({"capabilities":{}})
                }
                "initialized" => {
                    assert!(initialize_replied && !initialized);
                    initialized = true;
                    continue;
                }
                "shutdown" => {
                    assert!(initialized && !shutdown);
                    shutdown = true;
                    Value::Null
                }
                "exit" => {
                    assert!(shutdown && message.get("id").is_none());
                    if mode == "win-lsp-exit-needs-stdin-eof" {
                        // Model an input reader that also needs EOF before the
                        // server can stop; a complete exit frame is insufficient.
                        assert!(read_frame(&mut input, FrameLimits::default())
                            .expect("stdin must end at a frame boundary")
                            .is_none());
                        fs::write(dir.join("stdin-eof.ready"), b"verified").unwrap();
                    }
                    if mode == "win-lsp-graceful-eof-malformed" {
                        let mut output = io::stdout().lock();
                        output.write_all(b"Content-Length: 16\r\n\r\n{").unwrap();
                        output.flush().unwrap();
                    }
                    close_standard_handle(io::stdout().as_raw_handle());
                    closed.write_all(b"closed").unwrap();
                    closed.flush().unwrap();
                    match mode {
                        "win-lsp-exit-needs-stdin-eof" => return,
                        "win-lsp-graceful-eof-release" => {
                            let deadline = Instant::now() + Duration::from_secs(3);
                            while release.metadata().unwrap().len() == 0 {
                                assert!(
                                    Instant::now() < deadline,
                                    "graceful release gate did not open"
                                );
                                thread::sleep(Duration::from_millis(5));
                            }
                            return;
                        }
                        _ => idle(),
                    }
                }
                method => panic!("unexpected shutdown fixture method {method}"),
            };
            send(
                &mut io::stdout().lock(),
                json!({"jsonrpc":"2.0","id":message["id"],"result":result}),
            );
        }
    }

    fn hold_lifetime(dir: &Path, name: &str) -> File {
        let lifetime = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(dir.join(format!("{name}.lock")))
            .unwrap();
        // Atomic readiness record lets the agent suite open observation-only
        // process handles while this exact synthetic process is known live.
        let staging = dir.join(format!("{name}.pid-writing"));
        fs::write(&staging, process::id().to_string()).unwrap();
        fs::rename(staging, dir.join(format!("{name}.pid"))).unwrap();
        lifetime
    }

    fn spawn_descendant(dir: &Path, mode: &str) -> process::Child {
        Command::new(std::env::current_exe().unwrap())
            .arg(mode)
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(if mode.ends_with("-stderr") {
                Stdio::null()
            } else {
                Stdio::inherit()
            })
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap()
    }

    fn wait_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !path.is_file() {
            assert!(Instant::now() < deadline, "fixture gate did not open");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn idle() -> ! {
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn exclude_original_stdout_from_inheritance() {
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, SetHandleInformation, HANDLE_FLAG_INHERIT,
        };
        let handle = io::stdout().as_raw_handle();
        assert!(!handle.is_null() && handle as isize != -1);
        // SAFETY: this synthetic root owns its live inherited stdout handle.
        // No child has been spawned yet. Change only this handle's inheritance
        // bit, not permissions, buffering or process-wide standard-handle state.
        // Rust Command broadly inherits inheritable handles: stdout(NUL) alone
        // does not exclude this original pipe handle as an unrelated handle.
        assert_ne!(
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) },
            0
        );
        let mut flags = 0;
        // SAFETY: same live handle and a writable DWORD output.
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
    }

    fn close_standard_handle(handle: RawHandle) {
        assert!(!handle.is_null() && handle as isize != -1);
        // SAFETY: This synthetic process owns the inherited standard handle.
        // No Rust File/OwnedHandle owns it, no thread uses that stream, and all
        // temporary stdout/stderr locks have been dropped. Transfer ownership
        // exactly once to close it. The caller never accesses that stream
        // again and opens no new handles after closure (avoiding handle reuse).
        // Rust's global stdout/stderr objects do not close the standard handle
        // on Drop. The fixture watchdog only writes a separate file and exits.
        drop(unsafe { OwnedHandle::from_raw_handle(handle) });
    }
}
