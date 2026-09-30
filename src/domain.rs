use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const PLANNER_SCHEMA_VERSION: u32 = 1;
const MAX_ASSISTANT_MESSAGE: usize = 20_000;
const MAX_REQUIREMENT_LENGTH: usize = 3_000;
const MAX_ARCHITECTURE_TEXT: usize = 8_000;
const MAX_PLANNER_CHANGES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Architecture {
    pub overview: String,
    pub stack: Vec<StackChoice>,
    pub decisions: Vec<ArchitectureDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StackChoice {
    pub category: String,
    pub technology: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureDecision {
    pub topic: String,
    pub decision: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeletedRequirement {
    pub id: String,
    pub text: String,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub id: String,
    pub revision: i64,
    pub architecture: Option<Architecture>,
    pub messages: Vec<Message>,
    pub requirements: Vec<Requirement>,
    pub deleted_requirements: Vec<DeletedRequirement>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlannerResponse {
    pub schema_version: u32,
    pub message: String,
    pub architecture: Option<Architecture>,
    pub requirement_changes: Vec<RequirementChange>,
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequirementChange {
    Add { text: String },
    Update { id: String, text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectSnapshot {
    pub architecture: Architecture,
    pub requirements: Vec<Requirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub path: String,
    pub status: String,
    pub error: Option<String>,
    pub snapshot: ProjectSnapshot,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    Invalid(String),
    Conflict(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Conflict(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ValidationError {}

impl PlannerResponse {
    pub fn validate(&self, state: &SessionState) -> Result<(), ValidationError> {
        if self.schema_version != PLANNER_SCHEMA_VERSION {
            return Err(ValidationError::Invalid(format!(
                "unsupported planner schema version {}",
                self.schema_version
            )));
        }
        validate_text(&self.message, MAX_ASSISTANT_MESSAGE, "assistant message")?;
        if self.requirement_changes.len() > MAX_PLANNER_CHANGES {
            return Err(ValidationError::Invalid(
                "too many requirement changes".to_owned(),
            ));
        }
        if self.questions.len() > 20 {
            return Err(ValidationError::Invalid("too many questions".to_owned()));
        }
        for question in &self.questions {
            validate_text(question, 2_000, "question")?;
        }
        if let Some(architecture) = &self.architecture {
            validate_architecture(architecture)?;
        }

        let mut updated_ids = Vec::new();
        let mut active_texts = state
            .requirements
            .iter()
            .map(|requirement| {
                (
                    requirement.id.clone(),
                    normalize_requirement(&requirement.text),
                )
            })
            .collect::<HashMap<_, _>>();
        let deleted_texts = state
            .deleted_requirements
            .iter()
            .map(|requirement| normalize_requirement(&requirement.text))
            .collect::<Vec<_>>();
        for change in &self.requirement_changes {
            match change {
                RequirementChange::Add { text } => {
                    validate_text(text, MAX_REQUIREMENT_LENGTH, "requirement")?;
                    let normalized = normalize_requirement(text);
                    ensure_text_is_new(&normalized, &active_texts, &deleted_texts, None)?;
                    active_texts.insert(format!("\0new-{}", active_texts.len()), normalized);
                }
                RequirementChange::Update { id, text } => {
                    validate_text(text, MAX_REQUIREMENT_LENGTH, "requirement")?;
                    if updated_ids.iter().any(|updated_id| updated_id == id) {
                        return Err(ValidationError::Invalid(format!(
                            "requirement `{id}` is updated more than once"
                        )));
                    }
                    updated_ids.push(id.as_str());

                    if state
                        .deleted_requirements
                        .iter()
                        .any(|requirement| requirement.id == *id)
                    {
                        return Err(ValidationError::Invalid(format!(
                            "deleted requirement `{id}` cannot be changed by the agent"
                        )));
                    }
                    let existing = state
                        .requirements
                        .iter()
                        .find(|requirement| requirement.id == *id)
                        .ok_or_else(|| {
                            ValidationError::Invalid(format!(
                                "unknown requirement `{id}` in agent change"
                            ))
                        })?;
                    if existing.pinned {
                        return Err(ValidationError::Conflict(format!(
                            "pinned requirement `{id}` cannot be changed by the agent"
                        )));
                    }
                    let normalized = normalize_requirement(text);
                    ensure_text_is_new(&normalized, &active_texts, &deleted_texts, Some(id))?;
                    active_texts.insert(id.clone(), normalized);
                }
            }
        }
        Ok(())
    }
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn planner_prompt(state: &SessionState) -> Result<String, serde_json::Error> {
    let context = serde_json::to_string(state)?;
    Ok(format!(
        "You are the software project's planning agent. Do not create or edit files.\n\
         Respond with exactly one JSON object and no Markdown. The object must have fields: \
         schema_version (integer 1), message (string), architecture (object or null), \
         requirement_changes (array), and questions (array of strings). Architecture objects \
         have overview (string), stack (array of {{category, technology, rationale}}), and \
         decisions (array of {{topic, decision, rationale}}). Requirement changes are only \
         {{operation: \"add\", text: string}} or {{operation: \"update\", id: string, text: string}}.\n\
         The server assigns IDs. Never change or suggest an operation for a pinned requirement. \
         You cannot pin, unpin, delete, or restore requirements. Deleted requirements are listed \
         separately: do not recreate their intent, including by paraphrase. If the user asks to \
         restore one, explain that they must explicitly restore it through the client.\n\
         Keep proposed changes limited to what follows from the conversation. Do not replace \
         missing information with assumptions; ask questions instead.\n\
         Current canonical session state (including full conversation and tombstones):\n{context}"
    ))
}

pub fn implementation_prompt(snapshot: &ProjectSnapshot) -> Result<String, serde_json::Error> {
    let plan = serde_json::to_string_pretty(snapshot)?;
    Ok(format!(
        "Initialize this software project in the current empty directory. Follow the agreed \
         architecture, stack, and requirements below. Implement the project, add appropriate \
         tests and documentation, and run relevant checks. Do not alter the agreed scope.\n\n\
         Agreed project plan:\n{plan}"
    ))
}

fn validate_architecture(architecture: &Architecture) -> Result<(), ValidationError> {
    validate_text(
        &architecture.overview,
        MAX_ARCHITECTURE_TEXT,
        "architecture overview",
    )?;
    if architecture.stack.len() > 50 || architecture.decisions.len() > 100 {
        return Err(ValidationError::Invalid(
            "architecture contains too many entries".to_owned(),
        ));
    }
    for choice in &architecture.stack {
        validate_text(&choice.category, 200, "stack category")?;
        validate_text(&choice.technology, 500, "stack technology")?;
        validate_text(&choice.rationale, 2_000, "stack rationale")?;
    }
    for decision in &architecture.decisions {
        validate_text(&decision.topic, 500, "architecture topic")?;
        validate_text(&decision.decision, 2_000, "architecture decision")?;
        validate_text(&decision.rationale, 2_000, "decision rationale")?;
    }
    Ok(())
}

fn validate_text(text: &str, max_length: usize, label: &str) -> Result<(), ValidationError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::Invalid(format!("{label} cannot be empty")));
    }
    if text.chars().count() > max_length {
        return Err(ValidationError::Invalid(format!(
            "{label} exceeds {max_length} characters"
        )));
    }
    Ok(())
}

fn ensure_text_is_new(
    normalized: &str,
    active_texts: &HashMap<String, String>,
    deleted_texts: &[String],
    except_requirement_id: Option<&str>,
) -> Result<(), ValidationError> {
    let active_duplicate = active_texts
        .iter()
        .any(|(id, text)| Some(id.as_str()) != except_requirement_id && text == normalized);
    if active_duplicate {
        return Err(ValidationError::Invalid(
            "requirement duplicates an active requirement".to_owned(),
        ));
    }

    if deleted_texts.iter().any(|text| text == normalized) {
        return Err(ValidationError::Conflict(
            "requirement duplicates a deleted requirement; restore it explicitly instead"
                .to_owned(),
        ));
    }
    Ok(())
}

pub fn normalize_requirement(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        Architecture, PlannerResponse, Requirement, RequirementChange, SessionState,
        ValidationError, normalize_requirement,
    };

    fn state() -> SessionState {
        SessionState {
            id: "session".to_owned(),
            revision: 1,
            architecture: None,
            messages: vec![],
            requirements: vec![Requirement {
                id: "locked".to_owned(),
                text: "Use Rust".to_owned(),
                pinned: true,
            }],
            deleted_requirements: vec![],
            created_at: 1,
            updated_at: 1,
        }
    }

    fn response(changes: Vec<RequirementChange>) -> PlannerResponse {
        PlannerResponse {
            schema_version: 1,
            message: "Got it".to_owned(),
            architecture: None,
            requirement_changes: changes,
            questions: vec![],
        }
    }

    #[test]
    fn rejects_changes_to_pinned_requirements() {
        assert!(matches!(
            response(vec![RequirementChange::Update {
                id: "locked".to_owned(),
                text: "Use another language".to_owned(),
            }])
            .validate(&state()),
            Err(ValidationError::Conflict(_))
        ));
    }

    #[test]
    fn rejects_duplicate_requirement_text() {
        assert!(
            response(vec![RequirementChange::Add {
                text: " use   RUST ".to_owned(),
            }])
            .validate(&state())
            .is_err()
        );
    }

    #[test]
    fn rejects_duplicate_additions_within_one_agent_response() {
        assert!(
            response(vec![
                RequirementChange::Add {
                    text: "Store project data".to_owned(),
                },
                RequirementChange::Add {
                    text: " store   PROJECT data ".to_owned(),
                },
            ])
            .validate(&state())
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_in_structured_agent_response() {
        let input = r#"{
            "schema_version": 1,
            "message": "Okay",
            "architecture": null,
            "requirement_changes": [],
            "questions": [],
            "pinned": true
        }"#;

        assert!(serde_json::from_str::<PlannerResponse>(input).is_err());
    }

    #[test]
    fn rejects_empty_architecture_text() {
        let mut response = response(vec![]);
        response.architecture = Some(Architecture {
            overview: "  ".to_owned(),
            stack: vec![],
            decisions: vec![],
        });

        assert!(response.validate(&state()).is_err());
    }

    #[test]
    fn normalizes_case_and_whitespace() {
        assert_eq!(normalize_requirement("  Use  Rust\nnow "), "use rust now");
    }
}
