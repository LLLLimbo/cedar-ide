//! Standalone std-only fault peer, compiled by transport_tests on every platform.
//! It is never part of the shipped binaries and never opens a network socket.
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

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
fn main() {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().unwrap().to_string_lossy().into_owned();
    let dir = PathBuf::from(args.next().unwrap());
    fs::write(dir.join("started"), std::process::id().to_string()).unwrap();
    if mode == "eof_before_hello" {
        return;
    }
    if mode == "response_flood" {
        for _ in 0..100_000 {
            hello(1, 4);
        }
        return;
    }
    let mut input = io::stdin().lock();
    let mut line = String::new();
    let mut count = 0;
    loop {
        line.clear();
        if input.read_line(&mut line).unwrap_or(0) == 0 {
            fs::write(dir.join("eof"), b"orderly input close").unwrap();
            return;
        }
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(dir.join("requests"))
            .unwrap()
            .write_all(line.as_bytes())
            .unwrap();
        count += 1;
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
            hello(id, 4);
            if mode == "eof_between_requests" {
                return;
            }
            continue;
        }
        match mode.as_str() {
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
                let mut rest = Vec::new();
                input.read_to_end(&mut rest).unwrap();
                OpenOptions::new()
                    .append(true)
                    .open(dir.join("requests"))
                    .unwrap()
                    .write_all(&rest)
                    .unwrap();
                fs::write(dir.join("eof"), b"orderly input close").unwrap();
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
