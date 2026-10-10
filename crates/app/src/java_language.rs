//! Scoped Java configuration and inert, bounded shutdown reporting.
use cedar_protocol::Operation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ServerMode {
    #[default]
    Generic,
    Java,
}

#[derive(Default)]
pub(crate) struct JavaConfiguration {
    pub executable: String,
    pub distribution: String,
    pub data_directory: String,
}
impl JavaConfiguration {
    pub fn operation(&self) -> Result<Operation, String> {
        let paths = [&self.executable, &self.distribution, &self.data_directory];
        if paths.iter().any(|value| {
            value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
        }) {
            return Err("Enter the Java executable, JDT distribution and existing data directory on the workspace host".into());
        }
        // These are backend-host paths. The frontend must not canonicalize them
        // on its own computer or infer a different drive/platform spelling.
        Ok(Operation::LanguageStartJava {
            java_executable: self.executable.trim().into(),
            distribution: self.distribution.trim().into(),
            data_directory: self.data_directory.trim().into(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub(crate) enum StopStatus {
    Graceful,
    Forced,
    Error,
}
impl TryFrom<String> for StopStatus {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, &'static str> {
        match value.as_str() {
            "graceful" => Ok(Self::Graceful),
            "forced" => Ok(Self::Forced),
            "error" => Ok(Self::Error),
            _ => Err("invalid stop status"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub(crate) enum StopReason {
    RootExited,
    GraceExpired,
    Aborted,
    TransportFailure,
    WorkerPanicked,
}
impl TryFrom<String> for StopReason {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, &'static str> {
        match value.as_str() {
            "root_exited" => Ok(Self::RootExited),
            "grace_expired" => Ok(Self::GraceExpired),
            "aborted" => Ok(Self::Aborted),
            "transport_failure" => Ok(Self::TransportFailure),
            "worker_panicked" => Ok(Self::WorkerPanicked),
            _ => Err("invalid stop reason"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JavaRootExit {
    WindowsCode(u32),
    LinuxCode(u8),
    LinuxSignal(u8),
}

// The original Windows body is exactly six fields. A Linux body must carry
// its own platform and typed exit; neither branch may borrow the other's fields.
#[derive(Deserialize)]
#[serde(untagged)]
enum StopWire {
    Windows(WindowsStopWire),
    Linux(LinuxStopWire),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsStopWire {
    status: StopStatus,
    reason: StopReason,
    root_exit_code: u32,
    cleanup_joined: bool,
    shutdown_response_received: bool,
    exit_frame_completed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinuxStopWire {
    platform: String,
    status: StopStatus,
    reason: StopReason,
    root_exit: LinuxRootExit,
    cleanup_joined: bool,
    shutdown_response_received: bool,
    exit_frame_completed: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LinuxRootExit {
    Code { code: u8 },
    Signal { signal: u8 },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "StopWire")]
pub(crate) struct JavaStopOutcome {
    pub status: StopStatus,
    pub reason: StopReason,
    pub root_exit: JavaRootExit,
    pub cleanup_joined: bool,
    pub shutdown_response_received: bool,
    pub exit_frame_completed: bool,
}
impl TryFrom<StopWire> for JavaStopOutcome {
    type Error = &'static str;

    fn try_from(wire: StopWire) -> Result<Self, Self::Error> {
        let outcome = match wire {
            StopWire::Windows(wire) => Self {
                status: wire.status,
                reason: wire.reason,
                root_exit: JavaRootExit::WindowsCode(wire.root_exit_code),
                cleanup_joined: wire.cleanup_joined,
                shutdown_response_received: wire.shutdown_response_received,
                exit_frame_completed: wire.exit_frame_completed,
            },
            StopWire::Linux(wire) => {
                if wire.platform != "linux" {
                    return Err(Self::INVALID);
                }
                let root_exit = match wire.root_exit {
                    LinuxRootExit::Code { code } => JavaRootExit::LinuxCode(code),
                    LinuxRootExit::Signal { signal } if (1..=64).contains(&signal) => {
                        JavaRootExit::LinuxSignal(signal)
                    }
                    LinuxRootExit::Signal { .. } => return Err(Self::INVALID),
                };
                Self {
                    status: wire.status,
                    reason: wire.reason,
                    root_exit,
                    cleanup_joined: wire.cleanup_joined,
                    shutdown_response_received: wire.shutdown_response_received,
                    exit_frame_completed: wire.exit_frame_completed,
                }
            }
        };
        if !outcome.cleanup_joined
            || outcome.status == StopStatus::Graceful
                && (outcome.root_exit_code() != Some(0)
                    || outcome.reason != StopReason::RootExited
                    || !outcome.shutdown_response_received
                    || !outcome.exit_frame_completed)
        {
            return Err(Self::INVALID);
        }
        Ok(outcome)
    }
}
impl JavaStopOutcome {
    const INVALID: &'static str = "Java stop response did not verify process cleanup";

    pub fn parse(value: &Value) -> Result<Self, String> {
        // The protocol has already decoded a Value here, so duplicate raw JSON
        // keys cannot be recovered. Typed decoding still rejects mixed branches,
        // unknown fields and invalid witnesses; direct decoding rejects duplicates.
        let invalid = || Self::INVALID.to_owned();
        if value.get("stopped").and_then(Value::as_bool) != Some(true) {
            return Err(invalid());
        }
        serde_json::from_value(value.get("shutdown").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())
    }

    pub fn root_exit_code(&self) -> Option<u32> {
        match self.root_exit {
            JavaRootExit::WindowsCode(code) => Some(code),
            JavaRootExit::LinuxCode(code) => Some(u32::from(code)),
            JavaRootExit::LinuxSignal(_) => None,
        }
    }

    pub fn message(&self) -> String {
        match self.status {
            StopStatus::Graceful => "Java server exited gracefully (exit 0).".into(),
            StopStatus::Forced | StopStatus::Error => {
                let status = if self.status == StopStatus::Forced {
                    "stopped with forced cleanup"
                } else {
                    "stopped with cleanup errors"
                };
                let reason = match self.reason {
                    StopReason::RootExited => "process exited",
                    StopReason::GraceExpired => "shutdown grace period expired",
                    StopReason::Aborted => "session aborted",
                    StopReason::TransportFailure => "transport failed",
                    StopReason::WorkerPanicked => "cleanup worker failed",
                };
                let exit = match self.root_exit {
                    JavaRootExit::WindowsCode(code) => format!("exit {code}"),
                    JavaRootExit::LinuxCode(code) => format!("exit {code}"),
                    JavaRootExit::LinuxSignal(signal) => format!("signal {signal}"),
                };
                format!("Java server {status} ({reason}; {exit}).")
            }
        }
    }
}

#[cfg(test)]
#[path = "java_stop_tests.rs"]
mod stop_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn graceful() -> Value {
        json!({"stopped":true,"shutdown":{"status":"graceful","reason":"root_exited","root_exit_code":0,"cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true}})
    }
    #[test]
    fn java_configuration_preserves_backend_host_paths_and_requires_all_three() {
        let mut config = JavaConfiguration {
            executable: r"C:\Program Files\Java\bin\java.exe".into(),
            distribution: r"D:\JDT 雪".into(),
            data_directory: r"D:\Java data 雪".into(),
        };
        assert!(
            matches!(config.operation().unwrap(), Operation::LanguageStartJava { java_executable, distribution, data_directory } if java_executable == config.executable && distribution == config.distribution && data_directory == config.data_directory)
        );
        config.data_directory.clear();
        assert!(config.operation().is_err());
        config.data_directory = "bad\0path".into();
        assert!(config.operation().is_err());
    }
    #[test]
    fn graceful_stop_requires_every_native_protocol_and_cleanup_witness() {
        assert_eq!(
            JavaStopOutcome::parse(&graceful()).unwrap().message(),
            "Java server exited gracefully (exit 0)."
        );
        for (key, value) in [
            ("root_exit_code", json!(1)),
            ("cleanup_joined", json!(false)),
            ("shutdown_response_received", json!(false)),
            ("exit_frame_completed", json!(false)),
            ("reason", json!("grace_expired")),
        ] {
            let mut bad = graceful();
            bad["shutdown"][key] = value;
            assert!(JavaStopOutcome::parse(&bad).is_err());
        }
    }
    #[test]
    fn forced_and_error_stop_are_bounded_and_never_render_raw_payloads() {
        for status in ["forced", "error"] {
            let value = json!({"stopped":true,"private":"raw private payload","shutdown":{"status":status,"reason":"transport_failure","root_exit_code":259,"cleanup_joined":true,"shutdown_response_received":false,"exit_frame_completed":false}});
            let message = JavaStopOutcome::parse(&value).unwrap().message();
            assert!(!message.contains("gracefully"));
            assert!(!message.contains("private"));
            assert!(message.len() < 160);
        }
        for invalid in [
            Value::Null,
            json!({"stopped":true}),
            json!({"stopped":true,"shutdown":{"status":"private arbitrary text"}}),
        ] {
            assert!(JavaStopOutcome::parse(&invalid).is_err());
        }
    }
}
