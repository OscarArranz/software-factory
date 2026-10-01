mod agents;
mod api;
mod domain;
mod store;

use std::process::ExitCode;
use tokio::process::Command;

const USAGE: &str = "Usage:\n  sf serve [--host <host>] [--port <port>]\n  sf dev [--host <host>] [--port <port>] [--web-ui-port <port>]\n  sf --help";

#[derive(Debug)]
enum CliCommand {
    Help,
    Serve {
        host: String,
        port: u16,
    },
    Dev {
        host: String,
        port: u16,
        web_ui_port: u16,
    },
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
        Ok(CliCommand::Dev {
            host,
            port,
            web_ui_port,
        }) => match run_dev(&host, port, web_ui_port).await {
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

    if command != "serve" && command != "dev" {
        return Err(format!("unknown command `{command}`"));
    }

    let is_dev = command == "dev";
    let mut host = "127.0.0.1".to_owned();
    let mut port = 3000;
    let mut web_ui_port = 8080;
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
            "--web-ui-port" if is_dev => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for `--web-ui-port`".to_owned())?;
                web_ui_port = value
                    .parse::<u16>()
                    .map_err(|_| format!("invalid web UI port `{value}`"))?;
                if web_ui_port == 0 {
                    return Err("web UI port must be greater than zero".to_owned());
                }
            }
            unknown => return Err(format!("unknown option `{unknown}`")),
        }
    }

    if is_dev {
        Ok(CliCommand::Dev {
            host,
            port,
            web_ui_port,
        })
    } else {
        Ok(CliCommand::Serve { host, port })
    }
}

async fn run_dev(
    host: &str,
    port: u16,
    web_ui_port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let web_ui_directory = std::env::current_dir()?.join("crates/web-ui");
    if !web_ui_directory.join("Trunk.toml").is_file() {
        return Err("run `sf dev` from the software-factory repository root".into());
    }

    let api_base_url =
        std::env::var("SF_API_BASE_URL").unwrap_or_else(|_| web_ui_api_origin(host, port));
    let mut trunk = Command::new("trunk")
        .arg("serve")
        .arg("--port")
        .arg(web_ui_port.to_string())
        .env("SF_API_BASE_URL", api_base_url)
        .current_dir(web_ui_directory)
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("could not start Trunk; install Trunk and retry: {error}"))?;

    println!("sf web UI starting on http://127.0.0.1:{web_ui_port}");
    println!("sf API starting on http://{host}:{port}");

    tokio::select! {
        biased;
        result = tokio::signal::ctrl_c() => {
            let _ = trunk.kill().await;
            result?;
            Ok(())
        }
        result = api::serve(host, port) => {
            let _ = trunk.kill().await;
            result
        }
        result = trunk.wait() => {
            match result {
                Ok(status) => Err(format!("Trunk web UI server exited with {status}").into()),
                Err(error) => Err(error.into()),
            }
        }
    }
}

fn web_ui_api_origin(host: &str, port: u16) -> String {
    let host = match host {
        "0.0.0.0" => "127.0.0.1",
        "::" => "::1",
        host => host,
    };
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("http://{host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, parse_args, web_ui_api_origin};

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
    fn dev_defaults_to_localhost_ports() {
        assert!(matches!(
            parse_args(["dev"]).expect("dev command should parse"),
            CliCommand::Dev {
                host,
                port: 3000,
                web_ui_port: 8080,
            } if host == "127.0.0.1"
        ));
    }

    #[test]
    fn dev_parses_api_and_web_ui_ports() {
        assert!(matches!(
            parse_args([
                "dev",
                "--host",
                "0.0.0.0",
                "--port",
                "3001",
                "--web-ui-port",
                "8081"
            ])
            .expect("dev options should parse"),
            CliCommand::Dev {
                host,
                port: 3001,
                web_ui_port: 8081,
            } if host == "0.0.0.0"
        ));
    }

    #[test]
    fn rejects_zero_web_ui_port() {
        assert_eq!(
            parse_args(["dev", "--web-ui-port", "0"]).unwrap_err(),
            "web UI port must be greater than zero"
        );
    }

    #[test]
    fn dev_api_origin_uses_a_connectable_host_for_wildcard_bind_addresses() {
        assert_eq!(web_ui_api_origin("0.0.0.0", 3001), "http://127.0.0.1:3001");
        assert_eq!(web_ui_api_origin("::", 3001), "http://[::1]:3001");
    }

    #[test]
    fn dev_api_origin_formats_ipv6_hosts() {
        assert_eq!(web_ui_api_origin("::1", 3001), "http://[::1]:3001");
        assert_eq!(web_ui_api_origin("[::1]", 3001), "http://[::1]:3001");
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
