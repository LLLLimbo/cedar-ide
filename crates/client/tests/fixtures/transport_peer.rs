//! Standalone std-only fault peer, compiled by transport_tests on every platform.
//! It is never part of the shipped binaries and never opens a network socket.
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

fn emit(bytes: &[u8]) {
    let mut stdout = io::stdout().lock();
    if stdout.write_all(bytes).and_then(|_| stdout.flush()).is_err() {
        std::process::exit(0);
    }
}
fn hello(id: u64, protocol: u32) {
    emit(format!("{{\"id\":{id},\"result\":{{\"Ok\":{{\"type\":\"hello\",\"protocol\":{protocol},\"root\":\"/fixture\"}}}}}}\n").as_bytes());
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
        OpenOptions::new().append(true).create(true).open(dir.join("requests"))
            .unwrap().write_all(line.as_bytes()).unwrap();
        count += 1;
        let id: u64 = line.split("\"id\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
        if count == 1 {
            match mode.as_str() {
                "old_hello" => { hello(id, 3); continue; }
                "new_hello" => { hello(id, 5); continue; }
                "malformed_hello" => { emit(b"{\"id\":1,\"result\":{\"Ok\":{\"type\":\"hello\",\"protocol\":\"4\",\"root\":\"/\"}}}\n"); continue; }
                "missing_hello_field" => { emit(b"{\"id\":1,\"result\":{\"Ok\":{\"type\":\"hello\",\"protocol\":4}}}\n"); continue; }
                "wrong_hello_payload" => { emit(b"{\"id\":1,\"result\":{\"Ok\":{\"type\":\"entries\",\"entries\":[]}}}\n"); continue; }
                "wrong_id" => { hello(id + 1, 4); continue; }
                "truncated" => { emit(b"{\"id\":1,\"result\":"); return; }
                "oversized" => { emit(&vec![b'x'; 8 * 1024 * 1024 + 1]); return; }
                "bad_json" => { emit(b"not a JSON frame\n"); continue; }
                "stderr_flood" => {
                    let mut err = io::stderr().lock();
                    err.write_all(b"discarded-prefix").unwrap();
                    for _ in 0..4096 { err.write_all(&[b'x'; 1024]).unwrap(); }
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
            if mode == "eof_between_requests" { return; }
            continue;
        }
        match mode.as_str() {
            "eof_after_request" => return,
            "wrong_later_id" => hello(id - 1, 4),
            "stderr_flood" => { emit(b"invalid after diagnostics\n"); }
            "write_unknown" | "stalled" => {
                if mode == "write_unknown" {
                    // Model an applied mutation whose acknowledgement is lost.
                    fs::write(dir.join("committed"), b"one write applied").unwrap();
                }
                let mut rest = Vec::new();
                input.read_to_end(&mut rest).unwrap();
                OpenOptions::new().append(true).open(dir.join("requests"))
                    .unwrap().write_all(&rest).unwrap();
                fs::write(dir.join("eof"), b"orderly input close").unwrap();
                return;
            }
            "stubborn" => { thread::sleep(Duration::from_secs(10)); return; }
            _ => hello(id, 4),
        }
    }
}
