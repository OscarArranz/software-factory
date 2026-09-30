use std::{error::Error, fmt, io, path::Path, time::Duration};

pub mod opencode;

use crate::domain::PlannerResponse;

pub trait BuilderAgent: Send + Sync {
    fn plan(&self, working_directory: &Path, prompt: &str) -> Result<PlannerResponse, AgentError>;

    fn implement(&self, working_directory: &Path, prompt: &str) -> Result<(), AgentError>;
}

#[derive(Debug)]
pub enum AgentError {
    FailedToStart(io::Error),
    FailedToWait(io::Error),
    FailedToReadOutput(io::Error),
    UnsuccessfulExit { code: Option<i32>, stderr: String },
    InvalidOutput(String),
    TimedOut { timeout: Duration },
}

impl fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FailedToStart(error) => write!(formatter, "failed to start agent: {error}"),
            Self::FailedToWait(error) => {
                write!(formatter, "failed while waiting for agent: {error}")
            }
            Self::FailedToReadOutput(error) => {
                write!(formatter, "failed to read agent output: {error}")
            }
            Self::UnsuccessfulExit {
                code: Some(code),
                stderr,
            } => write_exit_error(formatter, *code, stderr),
            Self::UnsuccessfulExit { code: None, stderr } => {
                write_exit_error(formatter, -1, stderr)
            }
            Self::InvalidOutput(message) => write!(formatter, "invalid agent output: {message}"),
            Self::TimedOut { timeout } => {
                write!(
                    formatter,
                    "agent timed out after {} seconds",
                    timeout.as_secs()
                )
            }
        }
    }
}

fn write_exit_error(formatter: &mut fmt::Formatter<'_>, code: i32, stderr: &str) -> fmt::Result {
    if stderr.is_empty() {
        if code < 0 {
            formatter.write_str("agent exited unsuccessfully")
        } else {
            write!(formatter, "agent exited with status {code}")
        }
    } else if code < 0 {
        write!(formatter, "agent exited unsuccessfully: {stderr}")
    } else {
        write!(formatter, "agent exited with status {code}: {stderr}")
    }
}

impl Error for AgentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FailedToStart(error)
            | Self::FailedToWait(error)
            | Self::FailedToReadOutput(error) => Some(error),
            Self::UnsuccessfulExit { .. } | Self::InvalidOutput(_) | Self::TimedOut { .. } => None,
        }
    }
}
