//! Manual interoperability probe. This intentionally launches only a program the
//! operator supplies; it neither installs nor discovers language servers.
use cedar_language::{ClientOptions, LspClient, Position, ProcessConfig};
use serde_json::json;
use std::error::Error;
use std::time::Duration;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 5 {
        eprintln!("Usage: lsp_probe PROGRAM ROOT_URI DOCUMENT_URI LANGUAGE_ID SOURCE_PATH [SERVER_ARG ...]");
        std::process::exit(2);
    }
    let root = args[1].to_str().ok_or("root URI is not UTF-8")?;
    let document_uri = args[2].to_str().ok_or("document URI is not UTF-8")?;
    let language_id = args[3].to_str().ok_or("language ID is not UTF-8")?;
    let text = std::fs::read_to_string(&args[4])?;
    let mut config = ProcessConfig::new(&args[0]);
    config.args = args[5..].to_vec();
    let options = ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..ClientOptions::default()
    };
    let client = LspClient::spawn(config, options)?;
    let initialized = client.initialize(Some(root), json!({}))?;
    println!(
        "Initialize result: {}",
        serde_json::to_string_pretty(&initialized)?
    );
    client.did_open(document_uri, language_id, 1, &text)?;
    println!(
        "Hover at 0:0: {:?}",
        client.hover(document_uri, Position::default())
    );
    // A short sample only: diagnostics can require much longer indexing.
    for _ in 0..32 {
        match client.next_event(Duration::from_millis(100))? {
            Some(event) => println!("Event: {event:?}"),
            None => break,
        }
    }
    client.did_close(document_uri)?;
    client.shutdown()?;
    Ok(())
}
