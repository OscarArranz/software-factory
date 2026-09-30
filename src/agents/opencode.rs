use std::{
    collections::HashMap,
    io::{self, Read},
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::domain::PlannerResponse;

use super::{AgentError, BuilderAgent};

const MODEL: &str = "openai/gpt-6-luna";
const VARIANT: &str = "xhigh";
const DEFAULT_AGENT_TIMEOUT: Duration = Duration::from_secs(600);
const AGENT_TIMEOUT_ENV: &str = "SF_AGENT_TIMEOUT_SECS";

#[derive(Debug, Default)]
pub struct OpenCodeAgent;

impl OpenCodeAgent {
    fn command(&self, working_directory: &Path, agent: &str, prompt: &str) -> Command {
        let mut command = Command::new("opencode");
        command
            .arg("run")
            .arg("--format")
            .arg("json")
            .arg("--model")
            .arg(MODEL)
            .arg("--variant")
            .arg(VARIANT)
            .arg("--agent")
            .arg(agent)
            .arg("--dir")
            .arg(working_directory)
            .arg("--")
            .arg(prompt)
            .current_dir(working_directory);
        command
    }

    fn invoke(
        &self,
        working_directory: &Path,
        agent: &str,
        prompt: &str,
    ) -> Result<Output, AgentError> {
        run_command(
            self.command(working_directory, agent, prompt),
            configured_agent_timeout(),
        )
    }

    fn check_status(output: &Output) -> Result<(), AgentError> {
        if !output.status.success() {
            return Err(AgentError::UnsuccessfulExit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(())
    }
}

fn configured_agent_timeout() -> Duration {
    std::env::var(AGENT_TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_AGENT_TIMEOUT)
}

fn run_command(mut command: Command, timeout: Duration) -> Result<Output, AgentError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(AgentError::FailedToStart)?;
    let stdout = child.stdout.take().expect("piped stdout is available");
    let stderr = child.stderr.take().expect("piped stderr is available");
    let stdout_reader = read_output_in_background(stdout);
    let stderr_reader = read_output_in_background(stderr);
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                terminate_child(&mut child);
                let _ = join_output(stdout_reader);
                let _ = join_output(stderr_reader);
                return Err(AgentError::TimedOut { timeout });
            }
            Err(error) => {
                terminate_child(&mut child);
                let _ = join_output(stdout_reader);
                let _ = join_output(stderr_reader);
                return Err(AgentError::FailedToWait(error));
            }
        }
    };

    Ok(Output {
        status,
        stdout: join_output(stdout_reader)?,
        stderr: join_output(stderr_reader)?,
    })
}

fn read_output_in_background<R>(mut reader: R) -> JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_output(reader: JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>, AgentError> {
    reader
        .join()
        .map_err(|_| AgentError::FailedToReadOutput(io::Error::other("output reader panicked")))?
        .map_err(AgentError::FailedToReadOutput)
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

impl BuilderAgent for OpenCodeAgent {
    fn plan(&self, working_directory: &Path, prompt: &str) -> Result<PlannerResponse, AgentError> {
        let output = self.invoke(working_directory, "plan", prompt)?;
        Self::check_status(&output)?;
        let text = extract_text_events(&String::from_utf8_lossy(&output.stdout))?;
        serde_json::from_str(&text).map_err(|error| AgentError::InvalidOutput(error.to_string()))
    }

    fn implement(&self, working_directory: &Path, prompt: &str) -> Result<(), AgentError> {
        let output = self.invoke(working_directory, "build", prompt)?;
        Self::check_status(&output)
    }
}

fn extract_text_events(output: &str) -> Result<String, AgentError> {
    let mut chunks = Vec::<(Option<String>, String)>::new();
    let mut keyed_indices = HashMap::<String, usize>::new();
    let stream = serde_json::Deserializer::from_str(output).into_iter::<serde_json::Value>();

    for event in stream {
        let event = event.map_err(|error| AgentError::InvalidOutput(error.to_string()))?;
        let event_type = event.get("type").and_then(serde_json::Value::as_str);
        let part = event
            .get("part")
            .or_else(|| event.get("properties").and_then(|value| value.get("part")));
        let part_type = part
            .and_then(|value| value.get("type"))
            .and_then(serde_json::Value::as_str);
        if event_type != Some("text") && part_type != Some("text") {
            continue;
        }

        let text = part
            .and_then(|value| value.get("text"))
            .or_else(|| event.get("text"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AgentError::InvalidOutput("text event has no text".to_owned()))?;
        let id = part
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);

        if let Some(id) = id.as_ref() {
            if let Some(index) = keyed_indices.get(id) {
                chunks[*index].1 = text.to_owned();
                continue;
            }
            keyed_indices.insert(id.clone(), chunks.len());
        }
        chunks.push((id, text.to_owned()));
    }

    let text = chunks.into_iter().map(|(_, text)| text).collect::<String>();
    if text.trim().is_empty() {
        Err(AgentError::InvalidOutput(
            "OpenCode returned no assistant text".to_owned(),
        ))
    } else {
        Ok(text.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        process::Command,
        time::{Duration, Instant},
    };

    use super::{AgentError, OpenCodeAgent, extract_text_events, run_command};

    #[test]
    fn extracts_assistant_text_from_raw_json_events() {
        let output = r#"{"type":"step_start"}
{"type":"text","part":{"id":"p1","type":"text","text":"{\"schema_version\":"}}
{"type":"text","part":{"id":"p1","type":"text","text":"{\"schema_version\":1}"}}
{"type":"tool","part":{"type":"tool","text":"ignore this"}}"#;

        assert_eq!(
            extract_text_events(output).expect("text event should be extracted"),
            "{\"schema_version\":1}"
        );
    }

    #[test]
    fn extracts_event_properties_shape() {
        let output = r#"{"type":"message.part.updated","properties":{"part":{"id":"p1","type":"text","text":"hello"}}}"#;

        assert_eq!(
            extract_text_events(output).expect("nested text event should be extracted"),
            "hello"
        );
    }

    #[test]
    fn rejects_event_stream_without_assistant_text() {
        let output = r#"{"type":"tool","part":{"type":"tool"}}"#;

        assert!(extract_text_events(output).is_err());
    }

    #[test]
    fn command_selects_role_and_json_event_output() {
        let command = OpenCodeAgent.command(Path::new("/tmp/work"), "plan", "Return JSON");
        let args = command
            .get_args()
            .map(|arg| arg.to_str().expect("argument should be UTF-8"))
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "run",
                "--format",
                "json",
                "--model",
                "openai/gpt-6-luna",
                "--variant",
                "xhigh",
                "--agent",
                "plan",
                "--dir",
                "/tmp/work",
                "--",
                "Return JSON"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminates_an_agent_process_after_its_timeout() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();

        let result = run_command(command, Duration::from_millis(100));

        assert!(matches!(result, Err(AgentError::TimedOut { .. })));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn captures_output_from_a_completed_process() {
        let mut command = Command::new("printf");
        command.arg("agent output");

        let output =
            run_command(command, Duration::from_secs(2)).expect("short process should complete");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"agent output");
    }
}
