mod agents;

use std::process::ExitCode;

use agents::{Agent, AgentError, opencode::OpenCodeAgent};

const USAGE: &str = "Usage:\n  sf run <message>\n  sf --help";

#[derive(Debug)]
enum CliCommand {
    Help,
    Run(String),
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(CliCommand::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(CliCommand::Run(message)) => match OpenCodeAgent.run(&message) {
            Ok(()) => ExitCode::SUCCESS,
            Err(AgentError::UnsuccessfulExit { code: Some(code) }) => {
                ExitCode::from(code.clamp(1, u8::MAX.into()) as u8)
            }
            Err(AgentError::UnsuccessfulExit { code: None }) => {
                eprintln!("sf: agent terminated without an exit code");
                ExitCode::FAILURE
            }
            Err(error) => {
                eprintln!("sf: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("sf: {error}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_args<I, S>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let Some(command) = args.next() else {
        return Ok(CliCommand::Help);
    };

    if command == "--help" || command == "-h" {
        return Ok(CliCommand::Help);
    }

    if command != "run" {
        return Err(format!("unknown command `{command}`"));
    }

    let message = args.collect::<Vec<_>>().join(" ");
    if message.trim().is_empty() {
        return Err("missing message for `run`".to_owned());
    }

    Ok(CliCommand::Run(message))
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, parse_args};

    #[test]
    fn parses_run_message_and_joins_arguments() {
        let command =
            parse_args(["run", "Build", "the", "project"]).expect("run command should parse");

        assert!(matches!(command, CliCommand::Run(message) if message == "Build the project"));
    }

    #[test]
    fn displays_help_without_a_command() {
        assert!(matches!(
            parse_args(std::iter::empty::<String>()),
            Ok(CliCommand::Help)
        ));
    }

    #[test]
    fn rejects_a_run_without_a_message() {
        assert_eq!(
            parse_args(["run"]).unwrap_err(),
            "missing message for `run`"
        );
    }

    #[test]
    fn rejects_unknown_commands() {
        assert_eq!(
            parse_args(["build"]).unwrap_err(),
            "unknown command `build`"
        );
    }
}
