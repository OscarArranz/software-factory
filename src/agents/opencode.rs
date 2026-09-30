use std::process::Command;

use super::{Agent, AgentError};

const MODEL: &str = "openai/gpt-6-luna";
const VARIANT: &str = "xhigh";

#[derive(Debug, Default)]
pub struct OpenCodeAgent;

impl OpenCodeAgent {
    fn command(&self, message: &str) -> Command {
        let mut command = Command::new("opencode");
        command
            .arg("run")
            .arg("--model")
            .arg(MODEL)
            .arg("--variant")
            .arg(VARIANT)
            .arg("--")
            .arg(message);
        command
    }
}

impl Agent for OpenCodeAgent {
    fn run(&self, message: &str) -> Result<(), AgentError> {
        let status = self
            .command(message)
            .status()
            .map_err(AgentError::FailedToStart)?;

        if status.success() {
            Ok(())
        } else {
            Err(AgentError::UnsuccessfulExit {
                code: status.code(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OpenCodeAgent;

    #[test]
    fn command_uses_requested_model_variant_and_message() {
        let command = OpenCodeAgent.command("Build the project");
        let args = command
            .get_args()
            .map(|arg| arg.to_str().expect("arguments should be UTF-8"))
            .collect::<Vec<_>>();

        assert_eq!(command.get_program(), "opencode");
        assert_eq!(
            args,
            [
                "run",
                "--model",
                "openai/gpt-6-luna",
                "--variant",
                "xhigh",
                "--",
                "Build the project"
            ]
        );
    }
}
