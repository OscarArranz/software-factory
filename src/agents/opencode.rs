use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Output},
};

use crate::domain::PlannerResponse;

use super::{AgentError, BuilderAgent};

const MODEL: &str = "openai/gpt-6-luna";
const VARIANT: &str = "xhigh";

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
        self.command(working_directory, agent, prompt)
            .output()
            .map_err(AgentError::FailedToStart)
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
    use std::path::Path;

    use super::{OpenCodeAgent, extract_text_events};

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
}
