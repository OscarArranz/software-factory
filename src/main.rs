mod agents;
mod api;
mod domain;
mod store;

use std::process::ExitCode;

const USAGE: &str = "Usage:\n  sf serve [--host <host>] [--port <port>]\n  sf --help";

#[derive(Debug)]
enum CliCommand {
    Help,
    Serve { host: String, port: u16 },
}

#[tokio::main]
async fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(CliCommand::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(CliCommand::Serve { host, port }) => match api::serve(&host, port).await {
            Ok(()) => ExitCode::SUCCESS,
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

    if command != "serve" {
        return Err(format!("unknown command `{command}`"));
    }

    let mut host = "127.0.0.1".to_owned();
    let mut port = 3000;
    while let Some(option) = args.next() {
        match option.as_str() {
            "--help" | "-h" => return Ok(CliCommand::Help),
            "--host" => {
                host = args
                    .next()
                    .ok_or_else(|| "missing value for `--host`".to_owned())?;
                if host.trim().is_empty() {
                    return Err("host cannot be empty".to_owned());
                }
            }
            "--port" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for `--port`".to_owned())?;
                port = value
                    .parse::<u16>()
                    .map_err(|_| format!("invalid port `{value}`"))?;
                if port == 0 {
                    return Err("port must be greater than zero".to_owned());
                }
            }
            unknown => return Err(format!("unknown option `{unknown}`")),
        }
    }

    Ok(CliCommand::Serve { host, port })
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, parse_args};

    #[test]
    fn defaults_serve_to_localhost_and_port_3000() {
        assert!(matches!(
            parse_args(["serve"]).expect("serve command should parse"),
            CliCommand::Serve { host, port } if host == "127.0.0.1" && port == 3000
        ));
    }

    #[test]
    fn displays_help_without_a_command() {
        assert!(matches!(
            parse_args(std::iter::empty::<String>()),
            Ok(CliCommand::Help)
        ));
    }

    #[test]
    fn parses_host_and_port_options() {
        assert!(matches!(
            parse_args(["serve", "--host", "0.0.0.0", "--port", "8080"])
                .expect("serve options should parse"),
            CliCommand::Serve { host, port } if host == "0.0.0.0" && port == 8080
        ));
    }

    #[test]
    fn rejects_unknown_commands() {
        assert_eq!(
            parse_args(["build"]).unwrap_err(),
            "unknown command `build`"
        );
    }

    #[test]
    fn rejects_the_removed_run_command() {
        assert_eq!(
            parse_args(["run", "hello"]).unwrap_err(),
            "unknown command `run`"
        );
    }

    #[test]
    fn rejects_invalid_ports() {
        assert_eq!(
            parse_args(["serve", "--port", "70000"]).unwrap_err(),
            "invalid port `70000`"
        );
    }
}
