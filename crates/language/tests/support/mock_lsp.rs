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
    let mut audit = args.get(2).map(|path| {
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
                if mode == "delayed-initialize" {
                    thread::sleep(Duration::from_millis(250));
                }
                let mut capabilities = if mode == "no-capabilities" {
                    json!({})
                } else {
                    json!({"textDocumentSync":{"openClose":true,"change":if mode == "incremental" {2} else {1}}, "completionProvider":{"resolveProvider":true}, "definitionProvider":true,"hoverProvider":true})
                };
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
                Value::Null
            }
            "exit" => {
                assert!(shutdown);
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
