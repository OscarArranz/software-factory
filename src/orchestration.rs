use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use software_factory_api_types::{
    Message, MessageRole, Project, ProjectPlan, ProjectTask, ProjectWorkspace, TaskSpec, TaskStatus,
};

use crate::{
    api::AppState,
    domain::new_id,
    project_git,
    store::{Store, StoreError},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratorResponse {
    pub schema_version: u32,
    pub message: String,
    pub plan: Option<PlanProposal>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanProposal {
    pub summary: String,
    pub tasks: Vec<TaskSpec>,
}

impl OrchestratorResponse {
    pub fn validate(&self) -> Result<(), StoreError> {
        fn text(value: &str, max: usize) -> Result<(), StoreError> {
            if value.trim().is_empty() || value.chars().count() > max {
                return Err(StoreError::Invalid(
                    "empty or oversized orchestration text".into(),
                ));
            }
            Ok(())
        }
        if self.schema_version != 1 {
            return Err(StoreError::Invalid(
                "unsupported orchestrator schema version".into(),
            ));
        }
        text(&self.message, 20_000)?;
        if let Some(plan) = &self.plan {
            text(&plan.summary, 8_000)?;
            if plan.tasks.is_empty() || plan.tasks.len() > 50 {
                return Err(StoreError::Invalid("plans must contain 1-50 tasks".into()));
            }
            let mut titles = std::collections::HashSet::new();
            for (index, task) in plan.tasks.iter().enumerate() {
                text(&task.title, 200)?;
                text(&task.description, 8_000)?;
                if !titles.insert(task.title.trim().to_lowercase())
                    || task.acceptance_criteria.is_empty()
                    || task.acceptance_criteria.len() > 20
                    || task.verification_commands.is_empty()
                    || task.verification_commands.len() > 20
                    || task.dependencies.len() > index
                {
                    return Err(StoreError::Invalid(
                        "tasks need unique titles, acceptance criteria and verification commands"
                            .into(),
                    ));
                }
                for value in task
                    .acceptance_criteria
                    .iter()
                    .chain(&task.verification_commands)
                {
                    text(value, 2_000)?;
                }
                let mut dependencies = std::collections::HashSet::new();
                for dependency in &task.dependencies {
                    if *dependency >= index || !dependencies.insert(dependency) {
                        return Err(StoreError::Invalid(
                            "dependencies must uniquely reference earlier tasks in this plan"
                                .into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn message(role: MessageRole, content: String) -> Message {
    Message {
        id: new_id(),
        role,
        content,
        created_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    }
}

pub fn approve(workspace: &mut ProjectWorkspace, id: &str) -> Result<(), StoreError> {
    let plan = workspace
        .plans
        .iter_mut()
        .find(|plan| plan.id == id)
        .ok_or_else(|| StoreError::NotFound("plan was not found".into()))?;
    if plan.superseded {
        return Err(StoreError::Conflict(
            "this plan was superseded; confirm the current version".into(),
        ));
    }
    if !plan.approved {
        plan.approved = true;
        for task in workspace.tasks.iter_mut().filter(|task| task.plan_id == id) {
            if task.status == TaskStatus::PendingApproval {
                task.status = TaskStatus::Queued;
                task.activity
                    .push("Plan explicitly confirmed; queued for execution".into());
            }
        }
    }
    workspace.messages.push(message(MessageRole::Assistant, format!("Plan {id} confirmed. Its tasks are eligible for execution; dependencies will be respected.")));
    Ok(())
}

pub fn apply_response(
    workspace: &mut ProjectWorkspace,
    response: OrchestratorResponse,
) -> Result<(), StoreError> {
    response.validate()?;
    let mut content = response.message;
    if let Some(proposal) = response.plan {
        // An exact repeated proposal does not create a second executable plan.
        let repeated = workspace.plans.iter().filter(|p| !p.superseded).any(|p| {
            p.summary == proposal.summary
                && workspace
                    .tasks
                    .iter()
                    .filter(|t| t.plan_id == p.id)
                    .map(|t| &t.spec)
                    .eq(proposal.tasks.iter())
        });
        if !repeated {
            for plan in workspace
                .plans
                .iter_mut()
                .filter(|p| !p.approved && !p.superseded)
            {
                plan.superseded = true;
                for task in workspace.tasks.iter_mut().filter(|t| t.plan_id == plan.id) {
                    task.status = TaskStatus::Blocked;
                    task.error = Some("Plan superseded before approval".into());
                }
            }
            let id = new_id();
            content.push_str(&format!(
                "\n\nReview the plan and send `confirm {id}` to authorize this exact version."
            ));
            workspace.plans.push(ProjectPlan {
                id: id.clone(),
                summary: proposal.summary,
                approved: false,
                superseded: false,
            });
            workspace
                .tasks
                .extend(proposal.tasks.into_iter().map(|spec| ProjectTask {
                    id: new_id(),
                    plan_id: id.clone(),
                    spec,
                    status: TaskStatus::PendingApproval,
                    branch: None,
                    worktree: None,
                    base_commit: None,
                    commit: None,
                    error: None,
                    activity: vec!["Awaiting explicit plan confirmation".into()],
                }));
        }
    }
    workspace
        .messages
        .push(message(MessageRole::Assistant, content));
    Ok(())
}

pub fn prompt(
    project: &Project,
    workspace: &ProjectWorkspace,
) -> Result<String, serde_json::Error> {
    Ok(format!(
        "You are this project's software orchestrator. Read the repository to understand its current implementation, but never edit files or run mutating commands. \
         Discuss requested changes, clarify material unknowns and propose only agreed scope. Explain tradeoffs and relevant functional/non-functional requirements. \
         Return exactly JSON without Markdown: {{schema_version:1,message:string,plan:null or {{summary:string,tasks:[{{title:string,description:string,acceptance_criteria:[string],verification_commands:[string],dependencies:[integer]}}]}}}}. \
         Each task must be independently implementable with testable criteria and non-interactive verification commands run with sh in its worktree. Dependencies are zero-based indices of earlier tasks in this plan. \
         Separate independent tasks for parallel execution; tasks touching coupled code should have dependencies. \
         Never claim a task was executed or authorize execution: only the user's exact confirm command authorizes a plan. \
         If information is missing, ask questions and return plan:null. Do not duplicate existing queued/completed scope. \
         For failed/blocked tasks propose a follow-up plan if requested, using main as the starting point and referencing retained work where relevant. \
         Project:\n{}\nWorkspace:\n{}",
        serde_json::to_string(project)?,
        serde_json::to_string(workspace)?
    ))
}

pub fn mutate_task(
    state: &AppState,
    project_id: &str,
    task_id: &str,
    update: impl FnOnce(&mut ProjectTask),
) -> Result<(), String> {
    let mut store = state.store().map_err(|e| e.to_string())?;
    let mut workspace = store.workspace(project_id).map_err(|e| e.to_string())?;
    let revision = workspace.revision;
    let task = workspace
        .tasks
        .iter_mut()
        .find(|t| t.id == task_id)
        .ok_or("task not found")?;
    update(task);
    store
        .save_workspace(&mut workspace, revision)
        .map_err(|e| e.to_string())
}

fn transition(
    state: &AppState,
    project_id: &str,
    task_id: &str,
    status: TaskStatus,
    note: &str,
) -> Result<(), String> {
    mutate_task(state, project_id, task_id, |task| {
        task.status = status;
        task.activity.push(note.into());
    })
}

pub fn execute(state: &AppState, project: &Project, task: &ProjectTask) -> Result<(), String> {
    let directory = Path::new(&project.path);
    let integration_lock = state.integration_lock(&project.id)?;
    let (branch, worktree, base) = {
        let _guard = integration_lock.lock().map_err(|e| e.to_string())?;
        project_git::create_worktree(directory, &task.id)?
    };
    mutate_task(state, &project.id, &task.id, |task| {
        task.branch = Some(branch.clone());
        task.worktree = Some(worktree.to_string_lossy().into());
        task.base_commit = Some(base);
        task.activity
            .push("Created isolated branch and worktree from main".into());
    })?;
    let prompt = format!(
        "Implement only the approved task below in this worktree. Follow the project's architecture and functional/non-functional requirements. \
         Do not ask for terminal input. Make actual changes, add appropriate tests/documentation and run the agreed checks. \
         The Software Factory executor owns commits, branch integration and worktree cleanup: leave your changes in this worktree and do not switch branches or merge. \
         Project plan:\n{}\nApproved task:\n{}",
        serde_json::to_string(&project.snapshot).map_err(|e| e.to_string())?,
        serde_json::to_string(&task.spec).map_err(|e| e.to_string())?
    );
    state
        .agent
        .implement(&worktree, &prompt)
        .map_err(|e| e.to_string())?;
    if project.status != "completed"
        && !crate::api::project_contains_files(&worktree).map_err(|e| e.to_string())?
    {
        return Err(
            "implementation agent exited successfully without creating project files".into(),
        );
    }
    transition(
        state,
        &project.id,
        &task.id,
        TaskStatus::Verifying,
        "Running approved verification commands",
    )?;
    project_git::verify(&worktree, &task.spec.verification_commands)?;
    project_git::git(&worktree, &["add", "--all"])?;
    if project_git::git(&worktree, &["diff", "--cached", "--name-only"])?.is_empty() {
        return Err("implementer produced no changes for this task".into());
    }
    project_git::git(
        &worktree,
        &["commit", "-m", &format!("feat: {}", task.spec.title)],
    )?;
    let commit = project_git::git(&worktree, &["rev-parse", "HEAD"])?;
    mutate_task(state, &project.id, &task.id, |task| {
        task.commit = Some(commit);
    })?;
    transition(
        state,
        &project.id,
        &task.id,
        TaskStatus::Integrating,
        "Waiting for serialized main integration",
    )?;
    {
        let _guard = integration_lock.lock().map_err(|e| e.to_string())?;
        project_git::incorporate_main(&worktree)?;
        project_git::verify(&worktree, &task.spec.verification_commands)?;
        if !project_git::git(&worktree, &["status", "--porcelain"])?.is_empty() {
            return Err(
                "verification left uncommitted files; integration requires a clean worktree".into(),
            );
        }
        let commit = project_git::git(&worktree, &["rev-parse", "HEAD"])?;
        mutate_task(state, &project.id, &task.id, |task| {
            task.commit = Some(commit);
        })?;
        project_git::merge(directory, &branch)?;
        project_git::cleanup(directory, &worktree)?;
    }
    transition(
        state,
        &project.id,
        &task.id,
        TaskStatus::Completed,
        "Verified, committed, merged into main and removed worktree",
    )?;
    Ok(())
}

pub fn start_scheduler(state: AppState) {
    tokio::spawn(async move {
        loop {
            let worker_state = state.clone();
            match tokio::task::spawn_blocking(move || claim_ready(&worker_state)).await {
                Ok(Ok(jobs)) => {
                    for (project, task) in jobs {
                        let state = state.clone();
                        tokio::task::spawn_blocking(move || {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    execute(&state, &project, &task)
                                }));
                            let error = match result {
                                Ok(Ok(())) => return,
                                Ok(Err(error)) => error,
                                Err(_) => "implementation worker panicked".into(),
                            };
                            let _ = mutate_task(&state, &project.id, &task.id, |task| {
                                task.status = if task.status == TaskStatus::Integrating {
                                    TaskStatus::Blocked
                                } else {
                                    TaskStatus::Failed
                                };
                                task.error = Some(error.clone());
                                task.activity.push(error);
                            });
                        });
                    }
                }
                Ok(Err(error)) => eprintln!("sf: scheduler: {error}"),
                Err(error) => eprintln!("sf: scheduler worker: {error}"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn active(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Implementing | TaskStatus::Verifying | TaskStatus::Integrating
    )
}

fn claim_ready(state: &AppState) -> Result<Vec<(Project, ProjectTask)>, String> {
    let limit = std::env::var("SF_IMPLEMENTER_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2)
        .clamp(1, 16);
    let mut store = state.store().map_err(|e| e.to_string())?;
    let mut jobs = vec![];
    for project in store
        .list_projects()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|p| p.status == "completed")
    {
        let mut workspace = store.workspace(&project.id).map_err(|e| e.to_string())?;
        let revision = workspace.revision;
        let mut count = workspace.tasks.iter().filter(|t| active(&t.status)).count();
        let mut changed = false;
        for index in 0..workspace.tasks.len() {
            let task = &workspace.tasks[index];
            if task.status != TaskStatus::Queued
                || !workspace
                    .plans
                    .iter()
                    .any(|p| p.id == task.plan_id && p.approved && !p.superseded)
            {
                continue;
            }
            let siblings = workspace
                .tasks
                .iter()
                .filter(|t| t.plan_id == task.plan_id)
                .collect::<Vec<_>>();
            let dependencies = task
                .spec
                .dependencies
                .iter()
                .map(|i| siblings.get(*i).map(|t| &t.status))
                .collect::<Vec<_>>();
            if dependencies.iter().any(|status| {
                matches!(
                    status,
                    Some(TaskStatus::Blocked | TaskStatus::Failed) | None
                )
            }) {
                workspace.tasks[index].status = TaskStatus::Blocked;
                workspace.tasks[index].error = Some("A dependency failed or is blocked".into());
                workspace.tasks[index]
                    .activity
                    .push("Blocked by an unsuccessful dependency".into());
                changed = true;
            } else if count < limit
                && dependencies
                    .iter()
                    .all(|status| matches!(status, Some(TaskStatus::Completed)))
            {
                workspace.tasks[index].status = TaskStatus::Implementing;
                workspace.tasks[index]
                    .activity
                    .push("Claimed by implementation worker".into());
                jobs.push((project.clone(), workspace.tasks[index].clone()));
                count += 1;
                changed = true;
            }
        }
        if changed {
            store
                .save_workspace(&mut workspace, revision)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(jobs)
}

pub fn reconcile(store: &mut Store) -> Result<(), StoreError> {
    for project in store.list_projects()? {
        let mut workspace = store.workspace(&project.id)?;
        let revision = workspace.revision;
        let mut changed = false;
        for task in workspace.tasks.iter_mut().filter(|t| {
            active(&t.status) || (t.status == TaskStatus::Blocked && t.commit.is_some())
        }) {
            let directory = Path::new(&project.path);
            let merged = task.commit.as_ref().is_some_and(|commit| {
                project_git::git(directory, &["merge-base", "--is-ancestor", commit, "main"])
                    .is_ok()
            });
            if !merged && task.status == TaskStatus::Blocked {
                continue;
            }
            if merged {
                let cleanup = task
                    .worktree
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|path| path.exists())
                    .map(|path| project_git::cleanup(directory, &path))
                    .transpose();
                match cleanup {
                    Ok(_) => {
                        task.status = TaskStatus::Completed;
                        task.error = None;
                        task.activity.push(
                            "Restart recovery verified commit in main and cleaned worktree".into(),
                        );
                    }
                    Err(error) => {
                        task.status = TaskStatus::Blocked;
                        task.error = Some(format!(
                            "Commit merged; worktree cleanup requires attention: {error}"
                        ));
                    }
                }
            } else {
                task.status = TaskStatus::Blocked;
                task.error = Some(
                    "Service restarted during execution; retained work requires a follow-up plan"
                        .into(),
                );
                // Recover a worktree created immediately before its metadata was persisted.
                let path = directory
                    .parent()
                    .unwrap_or(directory)
                    .join(".sf-worktrees")
                    .join(&task.id);
                if path.exists() && task.worktree.is_none() {
                    task.worktree = Some(path.to_string_lossy().into());
                    task.branch = Some(format!("sf/task-{}", task.id));
                }
            }
            changed = true;
        }
        if changed {
            store.save_workspace(&mut workspace, revision)?;
        }
        if matches!(project.status.as_str(), "queued" | "running")
            && workspace.tasks.iter().any(|task| {
                task.spec.title == "Initialize project" && task.status == TaskStatus::Completed
            })
            && crate::api::project_contains_files(Path::new(&project.path)).unwrap_or(false)
        {
            store.update_project_status(&project.id, "completed", None)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ParallelAgent {
        barrier: std::sync::Barrier,
    }

    impl crate::agents::BuilderAgent for ParallelAgent {
        fn plan(
            &self,
            _: &Path,
            _: &str,
        ) -> Result<crate::domain::PlannerResponse, crate::agents::AgentError> {
            unreachable!()
        }
        fn implement(
            &self,
            directory: &Path,
            prompt: &str,
        ) -> Result<(), crate::agents::AgentError> {
            let (_, json) = prompt.rsplit_once("Approved task:\n").unwrap();
            let task: TaskSpec = serde_json::from_str(json).unwrap();
            if task.title != "dependent" {
                self.barrier.wait();
            }
            std::fs::write(directory.join(&task.title), &task.title).unwrap();
            Ok(())
        }
    }

    fn project_state() -> (tempfile::TempDir, AppState, Project) {
        let home = tempfile::tempdir().unwrap();
        let state = AppState::from_home(
            home.path(),
            std::sync::Arc::new(ParallelAgent {
                barrier: std::sync::Barrier::new(2),
            }),
        )
        .unwrap();
        let path = home.path().join("sf/projects/test");
        std::fs::create_dir(&path).unwrap();
        project_git::prepare(&path).unwrap();
        let mut store = state.store().unwrap();
        let session = store.create_session().unwrap();
        let response = crate::domain::PlannerResponse {
            schema_version: 2,
            message: "Plan".into(),
            architecture: Some(crate::domain::Architecture {
                overview: "Local service".into(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![crate::domain::RequirementChange::Add {
                text: "Store files".into(),
                kind: crate::domain::RequirementKind::Functional,
                acceptance_criteria: vec!["Files exist".into()],
            }],
            questions: vec![],
        };
        store
            .apply_planner_response(&session.id, session.revision, &response)
            .unwrap();
        let project = store
            .create_project_record(&session.id, "test", &path.to_string_lossy())
            .unwrap();
        let project = store
            .update_project_status(&project.id, "completed", None)
            .unwrap();
        drop(store);
        (home, state, project)
    }

    #[test]
    fn claims_atomically_runs_in_parallel_and_honors_dependencies() {
        let (_home, state, project) = project_state();
        let mut workspace = state.store().unwrap().workspace(&project.id).unwrap();
        let tasks = ["a", "b", "dependent"]
            .into_iter()
            .map(|name| TaskSpec {
                title: name.into(),
                description: format!("Create {name}"),
                acceptance_criteria: vec![format!("{name} exists")],
                verification_commands: vec![format!("test -f {name}")],
                dependencies: if name == "dependent" {
                    vec![0, 1]
                } else {
                    vec![]
                },
            })
            .collect();
        apply_response(
            &mut workspace,
            OrchestratorResponse {
                schema_version: 1,
                message: "Proposed".into(),
                plan: Some(PlanProposal {
                    summary: "Parallel work".into(),
                    tasks,
                }),
            },
        )
        .unwrap();
        state
            .store()
            .unwrap()
            .save_workspace(&mut workspace, 0)
            .unwrap();
        assert!(claim_ready(&state).unwrap().is_empty());
        let id = workspace.plans[0].id.clone();
        let revision = workspace.revision;
        approve(&mut workspace, &id).unwrap();
        state
            .store()
            .unwrap()
            .save_workspace(&mut workspace, revision)
            .unwrap();
        let jobs = claim_ready(&state).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(
            claim_ready(&state).unwrap().is_empty(),
            "no duplicate claims or early dependent execution"
        );
        let handles = jobs
            .into_iter()
            .map(|(project, task)| {
                let state = state.clone();
                std::thread::spawn(move || execute(&state, &project, &task).unwrap())
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        let jobs = claim_ready(&state).unwrap();
        assert_eq!(jobs.len(), 1);
        execute(&state, &jobs[0].0, &jobs[0].1).unwrap();
        let workspace = state.store().unwrap().workspace(&project.id).unwrap();
        for task in workspace.tasks {
            assert_eq!(task.status, TaskStatus::Completed);
            assert!(!Path::new(task.worktree.as_ref().unwrap()).exists());
            assert!(
                project_git::git(
                    Path::new(&project.path),
                    &[
                        "merge-base",
                        "--is-ancestor",
                        task.commit.as_ref().unwrap(),
                        "main"
                    ]
                )
                .is_ok()
            );
        }
        assert!(claim_ready(&state).unwrap().is_empty());
    }

    #[test]
    fn restart_recovers_merged_tasks_and_blocks_unmerged_work() {
        let (_home, state, project) = project_state();
        let directory = Path::new(&project.path);
        let mut workspace = state.store().unwrap().workspace(&project.id).unwrap();
        apply_response(&mut workspace, response()).unwrap();
        let task = &mut workspace.tasks[0];
        let (branch, worktree, base) = project_git::create_worktree(directory, &task.id).unwrap();
        std::fs::write(worktree.join("feature"), "feature").unwrap();
        project_git::git(&worktree, &["add", "--all"]).unwrap();
        project_git::git(&worktree, &["commit", "-m", "feature"]).unwrap();
        task.branch = Some(branch.clone());
        task.worktree = Some(worktree.to_string_lossy().into());
        task.base_commit = Some(base);
        task.commit = Some(project_git::git(&worktree, &["rev-parse", "HEAD"]).unwrap());
        task.status = TaskStatus::Integrating;
        project_git::merge(directory, &branch).unwrap();
        state
            .store()
            .unwrap()
            .save_workspace(&mut workspace, 0)
            .unwrap();
        reconcile(&mut state.store().unwrap()).unwrap();
        let workspace = state.store().unwrap().workspace(&project.id).unwrap();
        assert_eq!(workspace.tasks[0].status, TaskStatus::Completed);
        assert!(!worktree.exists());
        mutate_task(&state, &project.id, &workspace.tasks[0].id, |task| {
            task.status = TaskStatus::Implementing;
            task.commit = None;
        })
        .unwrap();
        reconcile(&mut state.store().unwrap()).unwrap();
        assert_eq!(
            state.store().unwrap().workspace(&project.id).unwrap().tasks[0].status,
            TaskStatus::Blocked
        );
    }

    fn workspace() -> ProjectWorkspace {
        ProjectWorkspace {
            project_id: "p".into(),
            revision: 0,
            messages: vec![],
            plans: vec![],
            tasks: vec![],
        }
    }
    fn response() -> OrchestratorResponse {
        OrchestratorResponse {
            schema_version: 1,
            message: "Proposed".into(),
            plan: Some(PlanProposal {
                summary: "Add feature".into(),
                tasks: vec![TaskSpec {
                    title: "Feature".into(),
                    description: "Implement feature".into(),
                    acceptance_criteria: vec!["Feature works".into()],
                    verification_commands: vec!["test -f feature".into()],
                    dependencies: vec![],
                }],
            }),
        }
    }
    #[test]
    fn plans_require_exact_confirmation_and_do_not_duplicate() {
        let mut w = workspace();
        apply_response(&mut w, response()).unwrap();
        assert_eq!(w.tasks[0].status, TaskStatus::PendingApproval);
        let id = w.plans[0].id.clone();
        assert!(approve(&mut w, "wrong-version").is_err());
        approve(&mut w, &id).unwrap();
        approve(&mut w, &id).unwrap();
        apply_response(&mut w, response()).unwrap();
        assert_eq!(w.tasks.len(), 1);
        assert_eq!(w.tasks[0].status, TaskStatus::Queued);
    }
    #[test]
    fn invalid_dependencies_and_stale_confirmations_are_rejected() {
        let mut invalid = response();
        invalid.plan.as_mut().unwrap().tasks[0].dependencies.push(0);
        let mut w = workspace();
        assert!(apply_response(&mut w, invalid).is_err());
        assert!(w.tasks.is_empty());
        apply_response(&mut w, response()).unwrap();
        let old = w.plans[0].id.clone();
        let mut new = response();
        new.plan.as_mut().unwrap().summary = "Revised".into();
        apply_response(&mut w, new).unwrap();
        assert!(approve(&mut w, &old).is_err());
    }
}
