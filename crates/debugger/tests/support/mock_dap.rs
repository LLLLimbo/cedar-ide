//! A deterministic adapter fixture. It never launches external programs.
use cedar_language::dap::{read_message, write_message, Message};
use cedar_language::framing::FrameLimits;
use serde_json::{json, Value};
use std::io::{self, BufReader, Write};
use std::thread;
use std::time::Duration;

struct Adapter {
    seq: u32,
}
impl Adapter {
    fn send(&mut self, message: Message) {
        write_message(&mut io::stdout().lock(), &message, FrameLimits::default()).unwrap();
    }
    fn response(&mut self, request_seq: u32, command: &str, body: Value) {
        self.seq += 1;
        self.send(Message::Response {
            seq: self.seq,
            request_seq,
            success: true,
            command: command.into(),
            message: None,
            body: Some(body),
        });
    }
    fn event(&mut self, event: &str, body: Value) {
        self.seq += 1;
        self.send(Message::Event {
            seq: self.seq,
            event: event.into(),
            body: Some(body),
        });
    }
}
fn main() {
    let scenario = std::env::args().nth(1).unwrap_or_default();
    if scenario == "stalled-stdin" {
        thread::sleep(Duration::from_secs(60));
        return;
    }
    let mut adapter = Adapter { seq: 1 };
    let mut reader = BufReader::new(io::stdin());
    let mut launch = None;
    let mut first_echo = None;
    while let Some(message) = read_message(&mut reader, FrameLimits::default()).unwrap() {
        let (seq, command, arguments) = match message {
            Message::Request {
                seq,
                command,
                arguments,
            } => (seq, command, arguments),
            Message::Response {
                request_seq,
                command,
                success,
                ..
            } => {
                assert!(!success);
                adapter.event(
                    "refusalObserved",
                    json!({"request_seq":request_seq,"command":command}),
                );
                continue;
            }
            _ => panic!("expected client request or rejection"),
        };
        match scenario.as_str() {
            "malformed" => {
                print!("Content-Length: 4\r\n\r\noops");
                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(60));
            }
            "oversize" => {
                print!("Content-Length: 99999999999\r\n\r\n");
                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(60));
            }
            "partial" => {
                print!("Content-Length: 100\r\n\r\n{{");
                return;
            }
            "eof" => return,
            "mismatch" => {
                adapter.response(seq, "wrongCommand", json!({}));
                continue;
            }
            "never" => continue,
            "overflow" => {
                for _ in 0..100 {
                    adapter.event("stopped", json!({"threadId":1}));
                }
                continue;
            }
            _ => {}
        }
        match command.as_str() {
            "initialize" => adapter.response(seq, &command, json!({"supportsConfigurationDoneRequest":true,"supportsTerminateRequest":true})),
            "launch" => { launch = Some(seq); adapter.event("initialized", json!({})); }
            "configurationDone" => {
                adapter.response(seq, &command, json!({}));
                adapter.response(launch.take().expect("launch must be pending"), "launch", json!({}));
                adapter.event("stopped", json!({"reason":"breakpoint","threadId":1,"allThreadsStopped":true}));
            }
            "setBreakpoints" => adapter.response(seq, &command, json!({"breakpoints":[{"id":1,"verified":true,"line":10}]})),
            "threads" => adapter.response(seq, &command, json!({"threads":[{"id":1,"name":"main"}]})),
            "stackTrace" => adapter.response(seq, &command, json!({"stackFrames":[{"id":10,"name":"main","line":10,"column":1,"source":{"path":"/owned/fixture.py"}}],"totalFrames":1})),
            "scopes" => adapter.response(seq, &command, json!({"scopes":[{"name":"Locals","variablesReference":20,"expensive":false}]})),
            "variables" => adapter.response(seq, &command, json!({"variables":[{"name":"answer","value":"42","variablesReference":0}]})),
            "continue" => { adapter.response(seq, &command, json!({"allThreadsContinued":true})); adapter.event("continued", json!({"threadId":1,"allThreadsContinued":true})); adapter.event("terminated", json!({})); }
            "disconnect" => { adapter.response(seq, &command, json!({})); return; }
            "outOfOrder" => {
                if let Some((old_seq, old_args)) = first_echo.take() {
                    adapter.response(seq, &command, arguments.unwrap_or(Value::Null));
                    adapter.response(old_seq, &command, old_args);
                } else { first_echo = Some((seq, arguments.unwrap_or(Value::Null))); }
            }
            "late" => { thread::sleep(Duration::from_millis(150)); adapter.response(seq, &command, json!({})); }
            "reverse" => {
                for (request_seq, name) in [(800,"runInTerminal"),(801,"startDebugging")] {
                    adapter.send(Message::Request { seq: request_seq, command:name.into(), arguments:Some(json!({"args":["never","execute"]})) });
                }
                adapter.response(seq, &command, json!({}));
            }
            "outputFlood" => {
                for _ in 0..300 { adapter.event("output", json!({"category":"stdout","output":"λ🦀0123456789\n"})); }
                adapter.event("output", json!({"category":"stdout","output":"THE-END"}));
                adapter.response(seq, &command, json!({}));
            }
            "fail" => adapter.send(Message::Response { seq: 999, request_seq:seq, command, success:false, message:Some("intentional".into()), body:Some(json!({"details":42})) }),
            _ => adapter.response(seq, &command, arguments.unwrap_or(json!({}))),
        }
    }
}
