//! Standalone std-only peer for the bounded Java refresh bridge tests. It emits
//! no diagnostics and never responds to validation notifications.
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
        let Some(id) = request.split("\"id\":").nth(1) else {
            // In particular, java/validateDocument receives neither a response
            // nor a publishDiagnostics event, even though the write succeeds.
            continue;
        };
        let id: u64 = id.split([',', '}']).next().unwrap().parse().unwrap();
        let result = if request.contains("\"method\":\"initialize\"") {
            initialize.as_str()
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
