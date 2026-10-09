//! Standalone std-only peer for bounded Java refresh and import bridge tests.
//! It emits no diagnostics and never responds to validation notifications.
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};

fn main() {
    let directory = PathBuf::from(std::env::args_os().nth(1).unwrap());
    let initialize = fs::read_to_string(directory.join("initialize.json")).unwrap();
    let mut audit = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("audit.jsonl"))
        .unwrap();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
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
        let request = String::from_utf8(bytes).unwrap();
        writeln!(audit, "{request}").unwrap();
        audit.flush().unwrap();
        if request.contains("\"method\":\"exit\"") {
            return;
        }
        // Audit client replies to server applyEdit, but never reply to a reply.
        if !request.contains("\"method\":") {
            continue;
        }
        let Some(id) = request.split("\"id\":").nth(1) else {
            // In particular, java/validateDocument receives neither a response
            // nor a publishDiagnostics event, even though the write succeeds.
            continue;
        };
        let id: u64 = id.split([',', '}']).next().unwrap().parse().unwrap();
        let organize = request.contains("\"method\":\"workspace/executeCommand\"");
        let symbols = request.contains("\"method\":\"workspace/symbol\"");
        if organize {
            if let Ok(apply) = fs::read_to_string(directory.join("server-apply-edit.json")) {
                write!(output, "Content-Length: {}\r\n\r\n{apply}", apply.len()).unwrap();
                output.flush().unwrap();
            }
        }
        let result = if request.contains("\"method\":\"initialize\"") {
            initialize.clone()
        } else if organize {
            fs::read_to_string(directory.join("organize-result.json"))
                .unwrap_or_else(|_| "{}".into())
        } else if symbols {
            fs::read_to_string(directory.join("symbols-result.json"))
                .unwrap_or_else(|_| "null".into())
        } else {
            "null".into()
        };
        let response = if organize && directory.join("organize-error.json").exists() {
            let error = fs::read_to_string(directory.join("organize-error.json")).unwrap();
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{error}}}")
        } else {
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}")
        };
        write!(
            output,
            "Content-Length: {}\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
        output.flush().unwrap();
    }
}
