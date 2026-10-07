//! Short-lived, synthetic executable for the opt-in Windows lifecycle suite.
//!
//! No shell, network, toolchain discovery, or persistent state is used. Every
//! invocation exits within five seconds, including descendants and crash owners.
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

const LIFETIME_CAP_EXIT: i32 = 124;
#[cfg(windows)]
const OWNER_CRASH_EXIT: i32 = 79;

fn main() {
    // Deliberately do not join: a hung pipe write or child cannot outlive the
    // cap. process::exit does not run Rust destructors, including job cleanup.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(5));
        process::exit(LIFETIME_CAP_EXIT);
    });
    if let Err(error) = run() {
        eprintln!("fixture error: {error}");
        process::exit(125);
    }
}

fn argument(args: &[String], index: usize) -> io::Result<&str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing fixture argument"))
}

fn run() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match argument(&args, 0)? {
        "marker" => {
            fs::write(argument(&args, 1)?, b"ran")?;
            println!("marker-ran");
        }
        "inspect" => {
            let cwd = env::current_dir()?;
            let cwd = cwd.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "fixture cwd is not UTF-8")
            })?;
            println!("cwd:{}", hex(cwd.as_bytes()));
            for arg in &args[1..] {
                println!("arg:{}", hex(arg.as_bytes()));
            }
        }
        "idle" => {
            println!("idle-stdout-ready");
            eprintln!("idle-stderr-ready");
            io::stdout().flush()?;
            io::stderr().flush()?;
            // Publish only after the final pipe I/O. In the owner-crash test,
            // readiness authorizes abrupt closure of the owner's read ends;
            // a later write failure must not masquerade as job termination.
            publish_pid(Path::new(argument(&args, 1)?))?;
            idle();
        }
        "tree-live" | "tree-exit" | "tree-flood" | "tree-coexist" => {
            let dir = Path::new(argument(&args, 1)?);
            if args[0] == "tree-coexist" {
                fs::write(dir.join("startup.waiting"), b"waiting for LSP startup")?;
                wait_for_file(Path::new(argument(&args, 2)?))?;
            }
            let _branch = spawn_fixture("branch", dir)?;
            wait_for_file(&dir.join("leaf.pid"))?;
            wait_for_file(&dir.join("branch.pid"))?;
            println!("root-ready");
            io::stdout().flush()?;
            // Tree-live/tree-exit readiness follows final pipe I/O, so reader
            // closure cannot simulate their cleanup. Tree-flood writes again
            // only after an explicit test-owned release gate; descendants idle.
            publish_pid(&dir.join("root.pid"))?;
            fs::write(dir.join("tree.ready"), b"ready")?;
            if args[0] == "tree-exit" {
                // Optional test-owned gate permits opening observation handles
                // while all three processes are known live before natural exit.
                if let Some(gate) = args.get(2) {
                    wait_for_file(Path::new(gate))?;
                }
                exit_with_code(23);
            }
            if args[0] == "tree-flood" {
                wait_for_file(Path::new(argument(&args, 2)?))?;
                flood(1024 * 1024)?;
            }
            idle();
        }
        "branch" => {
            let dir = Path::new(argument(&args, 1)?);
            let _leaf = spawn_fixture("leaf", dir)?;
            publish_pid(&dir.join("branch.pid"))?;
            idle();
        }
        "leaf" => {
            println!("leaf-stdout-ready");
            eprintln!("leaf-stderr-ready");
            io::stdout().flush()?;
            io::stderr().flush()?;
            publish_pid(&Path::new(argument(&args, 1)?).join("leaf.pid"))?;
            idle();
        }
        "flood" => {
            let bytes: usize = argument(&args, 1)?.parse().map_err(invalid_argument)?;
            if bytes > 2 * 1024 * 1024 {
                return Err(invalid_argument("flood limit exceeded"));
            }
            let code: u32 = argument(&args, 2)?.parse().map_err(invalid_argument)?;
            flood(bytes)?;
            exit_with_code(code);
        }
        "exit" => {
            let code: u32 = argument(&args, 1)?.parse().map_err(invalid_argument)?;
            exit_with_code(code);
        }
        "null-stdin" => {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            if !input.is_empty() {
                return Err(invalid_argument("task stdin was not null"));
            }
            println!("stdin-eof");
        }
        "stdin-echo" => {
            #[cfg(windows)]
            if let Some(sentinel) = args.get(1) {
                probe_sentinel(sentinel)?;
            }
            // Deliberately require EOF before emitting the payload. This proves
            // close_stdin closes the sole parent writer without a blocking flush.
            // Bound fixture memory even when the primitive under test misbehaves.
            let mut input = Vec::new();
            io::stdin().take(1024 * 1024 + 1).read_to_end(&mut input)?;
            if input.len() > 1024 * 1024 {
                return Err(invalid_argument("stdin fixture input limit exceeded"));
            }
            io::stdout().write_all(&input)?;
            println!("\nstdin-eof");
            eprintln!("stdin-bytes:{}", input.len());
        }
        #[cfg(windows)]
        "no-console" => {
            // SAFETY: GetConsoleWindow takes no pointers or ownership and only
            // queries this synthetic process's associated console window.
            // https://learn.microsoft.com/en-us/windows/console/getconsolewindow
            let window = unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() };
            if !window.is_null() {
                return Err(io::Error::other("stdio task has a console window"));
            }
            println!("console-window:null");
            eprintln!("console-stderr-ready");
        }
        "split-utf8" => {
            // Cross actual write boundaries in both streams, then publish the
            // final suffix before exit so terminal snapshots must finish drain.
            io::stdout().write_all(&[0xe9])?;
            io::stderr().write_all(&[0xf0, 0x9f])?;
            io::stdout().flush()?;
            io::stderr().flush()?;
            thread::sleep(Duration::from_millis(40));
            io::stdout().write_all(&[0x9b, 0xaa])?;
            io::stderr().write_all(&[0x9a, 0x80])?;
            println!("-stdout-final");
            eprintln!("-stderr-final");
        }
        "exact-streams" => {
            let out: usize = argument(&args, 1)?.parse().map_err(invalid_argument)?;
            let err: usize = argument(&args, 2)?.parse().map_err(invalid_argument)?;
            if out.max(err) > 2 * 1024 * 1024 {
                return Err(invalid_argument("stream limit exceeded"));
            }
            let writer = thread::spawn(move || -> io::Result<()> {
                let mut stdout = io::stdout().lock();
                write_bytes(&mut stdout, out, b'O')?;
                stdout.flush()
            });
            let mut stderr = io::stderr().lock();
            write_bytes(&mut stderr, err, b'E')?;
            stderr.flush()?;
            writer
                .join()
                .map_err(|_| io::Error::other("stdout thread panicked"))??;
        }
        #[cfg(windows)]
        "probe-sentinel" => probe_sentinel(argument(&args, 1)?)?,
        #[cfg(windows)]
        "crash-owner" => crash_owner(Path::new(argument(&args, 1)?), false)?,
        #[cfg(windows)]
        "crash-owner-piped" => crash_owner(Path::new(argument(&args, 1)?), true)?,
        _ => return Err(invalid_argument("unknown fixture mode")),
    }
    Ok(())
}

fn invalid_argument(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}

fn publish_pid(path: &Path) -> io::Result<()> {
    // Publish by rename so polling readers cannot see a partially written PID.
    let staging = path.with_extension("writing");
    fs::write(&staging, process::id().to_string())?;
    fs::rename(staging, path)
}

fn wait_for_file(path: &Path) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("fixture timed out waiting for {}", path.display()),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn spawn_fixture(mode: &str, dir: &Path) -> io::Result<process::Child> {
    // Explicit inheritance makes descendants retain both capture pipe writers.
    Command::new(env::current_exe()?)
        .arg(mode)
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

fn idle() -> ! {
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn flood(bytes: usize) -> io::Result<()> {
    let start = Arc::new(Barrier::new(2));
    let out_start = Arc::clone(&start);
    let out = thread::spawn(move || -> io::Result<()> {
        out_start.wait();
        let mut stdout = io::stdout().lock();
        write_bytes(&mut stdout, bytes, b'O')?;
        stdout.write_all(b"\nstdout-end\n")?;
        stdout.flush()
    });
    let err = thread::spawn(move || -> io::Result<()> {
        start.wait();
        let mut stderr = io::stderr().lock();
        write_bytes(&mut stderr, bytes, b'E')?;
        stderr.write_all(b"\nstderr-end\n")?;
        stderr.flush()
    });
    out.join()
        .map_err(|_| io::Error::other("stdout fixture thread panicked"))??;
    err.join()
        .map_err(|_| io::Error::other("stderr fixture thread panicked"))??;
    Ok(())
}

fn write_bytes(writer: &mut impl Write, bytes: usize, byte: u8) -> io::Result<()> {
    let block = [byte; 8192];
    let mut remaining = bytes;
    while remaining != 0 {
        let count = remaining.min(block.len());
        writer.write_all(&block[..count])?;
        remaining -= count;
    }
    Ok(())
}

fn exit_with_code(code: u32) -> ! {
    // Windows treats the i32 passed to Rust's exit as a DWORD bit pattern.
    // Unix fixture smoke checks use only portable codes; Windows tests check
    // 259 and high-bit values on the actual Windows executable.
    process::exit(code as i32)
}

#[cfg(windows)]
fn probe_sentinel(value: &str) -> io::Result<()> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::SetEvent;

    let value: usize = value.parse().map_err(invalid_argument)?;
    // SAFETY: This is a controlled negative-inheritance test. SetEvent accepts
    // an event handle and reports failure for an invalid handle. The value was
    // supplied by the test owning a live, inheritable event; we neither close
    // nor transfer ownership of it. A successful signal is observed by owner.
    // https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setevent
    let signaled = unsafe { SetEvent(value as HANDLE) };
    println!("sentinel-probed:{value}:{signaled}");
    Ok(())
}

#[cfg(windows)]
fn crash_owner(dir: &Path, piped: bool) -> io::Result<()> {
    use cedar_winprocess::{LaunchSpec, StdinWriteProgress, WindowsCommand, MAX_STDIN_WRITE_BYTES};

    let child_ready = dir.join("owned-child.pid");
    let spec = LaunchSpec {
        executable: env::current_exe()?,
        arguments: vec!["idle".into(), utf8_path(&child_ready)?],
        cwd: env::current_dir()?,
    };
    let mut child = if piped {
        WindowsCommand::spawn_suspended_with_piped_stdin(&spec)?
    } else {
        WindowsCommand::spawn_suspended(&spec)?
    };
    child.resume()?;
    wait_for_file(&child_ready)?;
    if piped {
        let bytes = vec![b'x'; MAX_STDIN_WRITE_BYTES];
        let mut pending = false;
        for _ in 0..16 {
            if child.begin_stdin_write(&bytes)? == StdinWriteProgress::Pending {
                pending = true;
                break;
            }
        }
        if !pending || child.poll_stdin_write()? != StdinWriteProgress::Pending {
            return Err(io::Error::other("crash-owner fixture did not block stdin"));
        }
        // Consume the two tiny readiness lines and arm both pending reads.
        child.capture_round(|_, _| {})?;
        child.capture_round(|_, _| {})?;
    }
    fs::write(dir.join("owner.ready"), b"ready")?;
    wait_for_file(&dir.join("owner.crash"))?;
    // No Rust destructors run. Only OS handle closure can destroy the inner
    // job. The test launches this owner outside its WindowsCommand jobs, so an
    // outer driver's explicit TerminateJobObject cannot mask this assertion.
    process::exit(OWNER_CRASH_EXIT);
}

#[cfg(windows)]
fn utf8_path(path: &Path) -> io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid_argument("fixture path is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::hex;

    #[test]
    fn protocol_hex_preserves_empty_unicode_and_special_arguments() {
        assert_eq!(hex(b""), "");
        assert_eq!(hex("雪\"\\".as_bytes()), "e99baa225c");
    }
}
