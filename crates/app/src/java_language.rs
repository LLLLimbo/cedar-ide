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
#[serde(rename_all = "snake_case")]
pub(crate) enum StopStatus {
    Graceful,
    Forced,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopReason {
    RootExited,
    GraceExpired,
    Aborted,
    TransportFailure,
    WorkerPanicked,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JavaStopOutcome {
    pub status: StopStatus,
    pub reason: StopReason,
    pub root_exit_code: u32,
    pub cleanup_joined: bool,
    pub shutdown_response_received: bool,
    pub exit_frame_completed: bool,
}
impl JavaStopOutcome {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let invalid = || "Java stop response did not verify process cleanup".to_owned();
        if value.get("stopped").and_then(Value::as_bool) != Some(true) {
            return Err(invalid());
        }
        let outcome: Self =
            serde_json::from_value(value.get("shutdown").cloned().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        if !outcome.cleanup_joined
            || outcome.status == StopStatus::Graceful
                && (outcome.root_exit_code != 0
                    || outcome.reason != StopReason::RootExited
                    || !outcome.shutdown_response_received
                    || !outcome.exit_frame_completed)
        {
            return Err(invalid());
        }
        Ok(outcome)
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
                format!(
                    "Java server {status} ({reason}; exit {}).",
                    self.root_exit_code
                )
            }
        }
    }
}

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
