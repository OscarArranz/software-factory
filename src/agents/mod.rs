use std::{error::Error, fmt, io};

pub mod opencode;

pub trait Agent {
    fn run(&self, message: &str) -> Result<(), AgentError>;
}

#[derive(Debug)]
pub enum AgentError {
    FailedToStart(io::Error),
    UnsuccessfulExit { code: Option<i32> },
}

impl fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FailedToStart(error) => write!(formatter, "failed to start agent: {error}"),
            Self::UnsuccessfulExit { code: Some(code) } => {
                write!(formatter, "agent exited with status {code}")
            }
            Self::UnsuccessfulExit { code: None } => {
                write!(formatter, "agent exited unsuccessfully")
            }
        }
    }
}

impl Error for AgentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FailedToStart(error) => Some(error),
            Self::UnsuccessfulExit { .. } => None,
        }
    }
}
