use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub kind: RequirementKind,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Functional,
    NonFunctional,
    #[default]
    Unclassified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeletedRequirement {
    pub id: String,
    pub text: String,
    pub deleted_at: i64,
    #[serde(default)]
    pub kind: RequirementKind,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
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
    #[serde(default)]
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub verification_commands: Vec<String>,
    /// Zero-based indices of earlier tasks in the same plan.
    pub dependencies: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    PendingApproval,
    Queued,
    Implementing,
    Verifying,
    Integrating,
    Completed,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTask {
    pub id: String,
    pub plan_id: String,
    pub spec: TaskSpec,
    pub status: TaskStatus,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub base_commit: Option<String>,
    pub commit: Option<String>,
    pub error: Option<String>,
    pub activity: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectPlan {
    pub id: String,
    pub summary: String,
    pub approved: bool,
    pub superseded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectWorkspace {
    pub project_id: String,
    pub revision: i64,
    pub messages: Vec<Message>,
    pub plans: Vec<ProjectPlan>,
    pub tasks: Vec<ProjectTask>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ErrorResponse {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::{CreateProjectRequest, SendMessageRequest};

    #[test]
    fn shared_requests_match_the_http_json_contract() {
        assert_eq!(
            serde_json::to_string(&SendMessageRequest {
                content: "Build an app".to_owned(),
            })
            .expect("message request should serialize"),
            r#"{"content":"Build an app"}"#
        );
        assert_eq!(
            serde_json::to_string(&CreateProjectRequest {
                name: "sample".to_owned(),
            })
            .expect("project request should serialize"),
            r#"{"name":"sample"}"#
        );
    }
}
