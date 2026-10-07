//! Dependency-free, synthetic run-profile fixture. It does nothing until run.
use std::io::Write;
use std::time::Duration;

fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            ch if ch <= '\u{1f}' => result.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir()?;
    println!(
        "{{\"cwd\":{},\"argv\":[{}]}}",
        json_string(&cwd.to_string_lossy()),
        args.iter()
            .map(|arg| json_string(arg))
            .collect::<Vec<_>>()
            .join(",")
    );
    std::io::stdout().flush()?;
    if args.iter().any(|arg| arg == "--touch-sentinel") {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open("sentinel.txt")?;
        file.write_all(b"Created only by the explicitly launched demo\n")?;
        file.sync_all()?;
        println!("Explicit sentinel created");
    }
    if let Some(index) = args.iter().position(|arg| arg == "--sleep-seconds") {
        let seconds: u64 = args
            .get(index + 1)
            .ok_or("Missing sleep duration")?
            .parse()?;
        if seconds > 120 {
            return Err("Fixture sleep is capped at 120 seconds".into());
        }
        println!("Ready for cancellation; waiting {seconds} seconds");
        std::io::stdout().flush()?;
        std::thread::sleep(Duration::from_secs(seconds));
    }
    println!("Demo completed");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Task fixture: {error}");
        std::process::exit(1);
    }
}
