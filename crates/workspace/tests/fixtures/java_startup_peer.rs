//! A finite std-only child used by private Workspace startup fixtures. It never
//! reads user repositories or impersonates an installed Java distribution.
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

fn await_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().unwrap());
    let mode = args.next().unwrap().into_string().unwrap();
    fs::write(
        directory.join(format!("{mode}.pid")),
        std::process::id().to_string(),
    )
    .unwrap();
    if mode == "task" {
        println!("independent-task-alive");
        io::stdout().flush().unwrap();
        await_file(&directory.join("task.release"));
        return;
    }
    if mode == "descendant" {
        // Intentionally retain inherited pipes while initialize is blocked.
        // The Windows owner must terminate this member of the same job too.
        await_file(&directory.join("descendant.release"));
        return;
    }
    // Establish this descendant before consuming any initialize request.
    let _descendant = if mode == "wait_init_tree" {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg(&directory)
            .arg("descendant")
            .spawn()
            .unwrap();
        await_file(&directory.join("descendant.pid"));
        Some(child)
    } else {
        None
    };
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut audit = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("audit.jsonl"))
        .unwrap();
    loop {
        let mut length = None;
        loop {
            let mut header = String::new();
            if input.read_line(&mut header).unwrap() == 0 {
                return;
            }
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let length = length.unwrap();
        assert!(length <= 1024 * 1024);
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).unwrap();
        let message = String::from_utf8(bytes).unwrap();
        writeln!(audit, "{message}").unwrap();
        audit.flush().unwrap();
        if message.contains("\"method\":\"exit\"") {
            return;
        }
        let Some(id) = message.split("\"id\":").nth(1) else {
            continue;
        };
        let id: u64 = id.split([',', '}']).next().unwrap().parse().unwrap();
        let result = if message.contains("\"method\":\"initialize\"") {
            fs::write(directory.join("initialize.received"), b"received").unwrap();
            if matches!(mode.as_str(), "wait_init" | "wait_init_tree") {
                await_file(&directory.join("initialize.release"));
            }
            if mode == "bad_init" {
                "{}"
            } else if mode == "imports_unsupported" {
                r#"{"capabilities":{"textDocumentSync":{"openClose":true,"change":1}},"serverInfo":{"name":"JDT Language Server (Standard)","version":"1.61.0-SNAPSHOT"},"cedar_java_organize_imports":true}"#
            } else {
                r#"{"capabilities":{"textDocumentSync":{"openClose":true,"change":1},"hoverProvider":true,"executeCommandProvider":{"commands":["java.edit.organizeImports"]}},"serverInfo":{"name":"JDT Language Server (Standard)","version":"1.61.0-SNAPSHOT"},"cedar_java_organize_imports":false}"#
            }
        } else {
            "null"
        };
        let response = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}");
        write!(
            output,
            "Content-Length: {}\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
        output.flush().unwrap();
    }
}
