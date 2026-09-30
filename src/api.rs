use std::{
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{
    agents::{AgentError, BuilderAgent, opencode::OpenCodeAgent},
    domain::{
        CreateProjectRequest, ErrorResponse, Project, SendMessageRequest, SessionState,
        implementation_prompt, planner_prompt,
    },
    store::{Store, StoreError},
};

#[derive(Clone)]
pub struct AppState {
    store: Arc<Mutex<Store>>,
    agent: Arc<dyn BuilderAgent>,
    data_directory: PathBuf,
    projects_directory: PathBuf,
}

impl AppState {
    fn from_home(home: &FsPath, agent: Arc<dyn BuilderAgent>) -> Result<Self, ApiError> {
        let data_directory = home.join("sf");
        let projects_directory = data_directory.join("projects");
        std::fs::create_dir_all(&projects_directory).map_err(ApiError::internal)?;
        let mut store = Store::open(&data_directory.join("software-factory.sqlite3"))?;
        reconcile_project_records(&mut store)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            agent,
            data_directory,
            projects_directory,
        })
    }

    fn store(&self) -> Result<MutexGuard<'_, Store>, ApiError> {
        self.store
            .lock()
            .map_err(|_| ApiError::internal("database lock is poisoned"))
    }
}

pub async fn serve(host: &str, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let state = AppState::from_home(&home, Arc::new(OpenCodeAgent))?;
    let app = router(state);
    let listener = TcpListener::bind((host, port)).await?;
    let address = listener.local_addr()?;
    println!("sf API listening on http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{session_id}", get(get_session))
        .route("/api/sessions/{session_id}/messages", post(send_message))
        .route(
            "/api/sessions/{session_id}/requirements/{requirement_id}/pin",
            put(pin_requirement).delete(unpin_requirement),
        )
        .route(
            "/api/sessions/{session_id}/requirements/{requirement_id}",
            delete(delete_requirement),
        )
        .route(
            "/api/sessions/{session_id}/deleted-requirements/{requirement_id}/restore",
            post(restore_requirement),
        )
        .route("/api/sessions/{session_id}/projects", post(create_project))
        .route("/api/projects", get(list_projects))
        .route("/api/projects/{project_id}", get(get_project))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
                    is_local_web_origin(origin)
                }))
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers([axum::http::header::CONTENT_TYPE]),
        )
        .with_state(state)
}

fn is_local_web_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(uri) = origin.parse::<Uri>() else {
        return false;
    };
    if uri.scheme_str() != Some("http") {
        return false;
    }
    let Some(host) = uri.host() else {
        return false;
    };
    let host = host.trim_end_matches('.');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let address = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    address
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn create_session(State(state): State<AppState>) -> Result<Response, ApiError> {
    Ok((StatusCode::CREATED, Json(state.store()?.create_session()?)).into_response())
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<Vec<SessionState>>, ApiError> {
    Ok(Json(state.store()?.list_sessions()?))
}

async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionState>, ApiError> {
    Ok(Json(state.store()?.get_session(&session_id)?))
}

async fn send_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SessionState>, ApiError> {
    let content = request.content.trim();
    if content.is_empty() {
        return Err(ApiError::invalid("message cannot be empty"));
    }
    if content.chars().count() > 20_000 {
        return Err(ApiError::invalid("message exceeds 20000 characters"));
    }

    let snapshot = state.store()?.add_user_message(&session_id, content)?;
    let prompt = planner_prompt(&snapshot).map_err(ApiError::internal)?;
    let agent = Arc::clone(&state.agent);
    let working_directory = state.data_directory.clone();
    let response = tokio::task::spawn_blocking(move || agent.plan(&working_directory, &prompt))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::agent)?;

    let final_state = state
        .store()?
        .apply_planner_response(&session_id, snapshot.revision, &response)
        .map_err(|error| match error {
            StoreError::Invalid(message) => ApiError::agent(AgentError::InvalidOutput(message)),
            error => ApiError::from(error),
        })?;
    Ok(Json(final_state))
}

async fn pin_requirement(
    State(state): State<AppState>,
    Path((session_id, requirement_id)): Path<(String, String)>,
) -> Result<Json<SessionState>, ApiError> {
    Ok(Json(state.store()?.set_requirement_pinned(
        &session_id,
        &requirement_id,
        true,
    )?))
}

async fn unpin_requirement(
    State(state): State<AppState>,
    Path((session_id, requirement_id)): Path<(String, String)>,
) -> Result<Json<SessionState>, ApiError> {
    Ok(Json(state.store()?.set_requirement_pinned(
        &session_id,
        &requirement_id,
        false,
    )?))
}

async fn delete_requirement(
    State(state): State<AppState>,
    Path((session_id, requirement_id)): Path<(String, String)>,
) -> Result<Json<SessionState>, ApiError> {
    Ok(Json(
        state
            .store()?
            .delete_requirement(&session_id, &requirement_id)?,
    ))
}

async fn restore_requirement(
    State(state): State<AppState>,
    Path((session_id, requirement_id)): Path<(String, String)>,
) -> Result<Json<SessionState>, ApiError> {
    Ok(Json(
        state
            .store()?
            .restore_requirement(&session_id, &requirement_id)?,
    ))
}

async fn create_project(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(request): Json<CreateProjectRequest>,
) -> Result<Response, ApiError> {
    let name = request.name.trim();
    validate_project_name(name)?;
    let directory = state.projects_directory.join(name);
    std::fs::create_dir_all(&state.projects_directory).map_err(ApiError::internal)?;
    std::fs::create_dir(&directory).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            ApiError::conflict("project destination already exists")
        } else {
            ApiError::internal(error)
        }
    })?;

    let project =
        match state
            .store()?
            .create_project_record(&session_id, name, &directory.to_string_lossy())
        {
            Ok(project) => project,
            Err(error) => {
                let _ = std::fs::remove_dir(&directory);
                return Err(ApiError::from(error));
            }
        };

    let background_state = state.clone();
    let background_project = project.clone();
    let failure_state = state.clone();
    let failed_project_id = project.id.clone();
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            execute_project(background_state, background_project)
        })
        .await;
        if let Err(error) = result {
            eprintln!("sf: project worker failed: {error}");
            mark_project_failed(&failure_state, &failed_project_id, &error.to_string());
        }
    });

    Ok((StatusCode::ACCEPTED, Json(project)).into_response())
}

fn validate_project_name(name: &str) -> Result<(), ApiError> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= 100
        && name.len() <= 200
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|character| character.is_control() || character == '/' || character == '\\');
    if valid {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "project name must be a single path component of at most 100 characters",
        ))
    }
}

fn execute_project(state: AppState, project: Project) {
    let start_result = state.store().and_then(|mut store| {
        store
            .update_project_status(&project.id, "running", None)
            .map(|_| ())
            .map_err(ApiError::from)
    });
    if let Err(error) = start_result {
        eprintln!(
            "sf: could not mark project {} as running: {error}",
            project.id
        );
        mark_project_failed(&state, &project.id, &error.to_string());
        return;
    }

    let prompt = match implementation_prompt(&project.snapshot) {
        Ok(prompt) => prompt,
        Err(error) => {
            mark_project_failed(&state, &project.id, &error.to_string());
            return;
        }
    };
    let agent = Arc::clone(&state.agent);
    let directory = PathBuf::from(&project.path);
    let result = agent.implement(&directory, &prompt);
    match result {
        Ok(()) => {
            match project_contains_files(&directory) {
                Ok(true) => {}
                Ok(false) => {
                    mark_project_failed(
                        &state,
                        &project.id,
                        "implementation agent exited successfully without creating project files",
                    );
                    return;
                }
                Err(error) => {
                    mark_project_failed(
                        &state,
                        &project.id,
                        &format!("could not verify generated project files: {error}"),
                    );
                    return;
                }
            }
            if let Err(error) = state.store().and_then(|mut store| {
                store
                    .update_project_status(&project.id, "completed", None)
                    .map(|_| ())
                    .map_err(ApiError::from)
            }) {
                eprintln!(
                    "sf: could not mark project {} complete: {error}",
                    project.id
                );
            }
        }
        Err(error) => mark_project_failed(&state, &project.id, &error.to_string()),
    }
}

fn project_contains_files(directory: &FsPath) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            return Ok(true);
        }
        if file_type.is_dir() && project_contains_files(&entry.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reconcile_project_records(store: &mut Store) -> Result<(), StoreError> {
    store.fail_interrupted_projects()?;
    for project in store.list_projects()? {
        if project.status != "completed" {
            continue;
        }
        let error = match project_contains_files(FsPath::new(&project.path)) {
            Ok(true) => continue,
            Ok(false) => "previous implementation completed without creating project files".into(),
            Err(error) => format!("could not verify previously completed project files: {error}"),
        };
        store.update_project_status(&project.id, "failed", Some(&error))?;
    }
    Ok(())
}

fn mark_project_failed(state: &AppState, project_id: &str, message: &str) {
    if let Err(error) = state.store().and_then(|mut store| {
        store
            .update_project_status(project_id, "failed", Some(message))
            .map(|_| ())
            .map_err(ApiError::from)
    }) {
        eprintln!("sf: could not record project failure: {error}");
    }
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(state.store()?.list_projects()?))
}

async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Project>, ApiError> {
    Ok(Json(state.store()?.get_project(&project_id)?))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn agent(error: AgentError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound(message) => Self {
                status: StatusCode::NOT_FOUND,
                message,
            },
            StoreError::Conflict(message) => Self::conflict(message),
            StoreError::Invalid(message) => Self::invalid(message),
            StoreError::Database(error) => Self::internal(error),
            StoreError::Serialization(error) => Self::internal(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response<Body> {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{
        agents::{AgentError, BuilderAgent},
        domain::{Architecture, PlannerResponse, Project, RequirementChange, SessionState},
        store::Store,
    };

    use super::{AppState, reconcile_project_records, router};

    #[derive(Clone)]
    struct FakeAgent {
        response: Arc<Mutex<Option<PlannerResponse>>>,
        implementation_calls: Arc<Mutex<Vec<(PathBuf, String)>>>,
        create_project_files: bool,
    }

    impl BuilderAgent for FakeAgent {
        fn plan(
            &self,
            _working_directory: &Path,
            _prompt: &str,
        ) -> Result<PlannerResponse, AgentError> {
            Ok(self
                .response
                .lock()
                .expect("fake response lock should work")
                .take()
                .expect("test response should be set"))
        }

        fn implement(&self, _working_directory: &Path, _prompt: &str) -> Result<(), AgentError> {
            self.implementation_calls
                .lock()
                .expect("fake implementation lock should work")
                .push((_working_directory.to_path_buf(), _prompt.to_owned()));
            if self.create_project_files {
                std::fs::write(_working_directory.join("README.md"), "# Generated project")
                    .expect("fake agent should write a generated project file");
            }
            Ok(())
        }
    }

    fn fake_agent(response: PlannerResponse) -> FakeAgent {
        FakeAgent {
            response: Arc::new(Mutex::new(Some(response))),
            implementation_calls: Arc::new(Mutex::new(Vec::new())),
            create_project_files: true,
        }
    }

    fn state_with_agent(agent: FakeAgent, home: &Path) -> AppState {
        std::fs::create_dir_all(home).expect("temporary home should exist");
        let data_directory = home.join("sf");
        let projects_directory = data_directory.join("projects");
        std::fs::create_dir_all(&projects_directory).expect("projects directory should exist");
        let store = Store::open(&data_directory.join("test.sqlite3"))
            .expect("temporary database should open");
        AppState {
            store: Arc::new(Mutex::new(store)),
            agent: Arc::new(agent),
            data_directory,
            projects_directory,
        }
    }

    async fn request(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> axum::response::Response {
        let mut builder = Request::builder().method(method).uri(uri);
        let body = if let Some(body) = body {
            builder = builder.header("content-type", "application/json");
            Body::from(body.to_string())
        } else {
            Body::empty()
        };
        app.oneshot(builder.body(body).expect("request should build"))
            .await
            .expect("router should handle request")
    }

    async fn json_body(response: axum::response::Response) -> Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("response body should read")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("response should be JSON")
    }

    #[tokio::test]
    async fn creates_sessions_and_persists_agent_turns() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let agent = fake_agent(PlannerResponse {
            schema_version: 1,
            message: "Let's use a local database.".to_owned(),
            architecture: Some(Architecture {
                overview: "A local project".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Persist user data".to_owned(),
            }],
            questions: vec![],
        });
        let app = router(state_with_agent(agent, home.path()));
        let created = request(app.clone(), "POST", "/api/sessions", None).await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let session: SessionState = serde_json::from_value(json_body(created).await)
            .expect("created response should be a session");

        let response = request(
            app.clone(),
            "POST",
            &format!("/api/sessions/{}/messages", session.id),
            Some(json!({ "content": "Create a local app" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let updated: SessionState = serde_json::from_value(json_body(response).await)
            .expect("message response should be a session");
        assert_eq!(updated.messages.len(), 2);
        assert_eq!(updated.requirements[0].text, "Persist user data");

        let fetched = request(
            app.clone(),
            "GET",
            &format!("/api/sessions/{}", session.id),
            None,
        )
        .await;
        let fetched: SessionState = serde_json::from_value(json_body(fetched).await)
            .expect("fetched response should be a session");
        assert_eq!(fetched.revision, updated.revision);

        let listed = request(app, "GET", "/api/sessions", None).await;
        let listed: Vec<SessionState> = serde_json::from_value(json_body(listed).await)
            .expect("session list should deserialize");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, session.id);
    }

    #[tokio::test]
    async fn validates_project_names_without_touching_outside_paths() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let app = router(state_with_agent(
            fake_agent(PlannerResponse {
                schema_version: 1,
                message: "Ready".to_owned(),
                architecture: None,
                requirement_changes: vec![],
                questions: vec![],
            }),
            home.path(),
        ));
        let created = request(app.clone(), "POST", "/api/sessions", None).await;
        let session: SessionState = serde_json::from_value(json_body(created).await)
            .expect("created response should be a session");
        let response = request(
            app,
            "POST",
            &format!("/api/sessions/{}/projects", session.id),
            Some(json!({ "name": "../outside" })),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!home.path().join("outside").exists());
    }

    async fn create_planned_session(app: &axum::Router) -> SessionState {
        let created = request(app.clone(), "POST", "/api/sessions", None).await;
        let session: SessionState = serde_json::from_value(json_body(created).await)
            .expect("created response should be a session");
        let response = request(
            app.clone(),
            "POST",
            &format!("/api/sessions/{}/messages", session.id),
            Some(json!({ "content": "Build a local project" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_value(json_body(response).await)
            .expect("planner response should return a session")
    }

    fn project_plan_response() -> PlannerResponse {
        PlannerResponse {
            schema_version: 1,
            message: "Architecture and requirement captured".to_owned(),
            architecture: Some(Architecture {
                overview: "A small local service".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Persist project data".to_owned(),
            }],
            questions: vec![],
        }
    }

    #[tokio::test]
    async fn project_creation_runs_agent_and_persists_snapshot() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let fake = fake_agent(project_plan_response());
        let calls = Arc::clone(&fake.implementation_calls);
        let app = router(state_with_agent(fake, home.path()));
        let session = create_planned_session(&app).await;
        let response = request(
            app.clone(),
            "POST",
            &format!("/api/sessions/{}/projects", session.id),
            Some(json!({ "name": "sample-project" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let queued: crate::domain::Project = serde_json::from_value(json_body(response).await)
            .expect("project response should be a project record");

        let mut current = queued.clone();
        for _ in 0..100 {
            let response = request(
                app.clone(),
                "GET",
                &format!("/api/projects/{}", queued.id),
                None,
            )
            .await;
            current = serde_json::from_value(json_body(response).await)
                .expect("project polling response should deserialize");
            if current.status == "completed" || current.status == "failed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(current.status, "completed");
        assert!(Path::new(&current.path).is_dir());
        assert_eq!(
            current.snapshot.requirements[0].text,
            "Persist project data"
        );
        let calls = calls.lock().expect("fake implementation lock should work");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, PathBuf::from(&current.path));
        assert!(calls[0].1.contains("Persist project data"));
    }

    #[tokio::test]
    async fn successful_agent_exit_without_files_marks_project_failed() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let mut fake = fake_agent(project_plan_response());
        fake.create_project_files = false;
        let app = router(state_with_agent(fake, home.path()));
        let session = create_planned_session(&app).await;
        let response = request(
            app.clone(),
            "POST",
            &format!("/api/sessions/{}/projects", session.id),
            Some(json!({ "name": "empty-project" })),
        )
        .await;
        let queued: Project = serde_json::from_value(json_body(response).await)
            .expect("project response should deserialize");

        let mut current = queued.clone();
        for _ in 0..100 {
            let response = request(
                app.clone(),
                "GET",
                &format!("/api/projects/{}", queued.id),
                None,
            )
            .await;
            current = serde_json::from_value(json_body(response).await)
                .expect("project poll response should deserialize");
            if current.status == "failed" || current.status == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(current.status, "failed");
        assert_eq!(
            current.error.as_deref(),
            Some("implementation agent exited successfully without creating project files")
        );
        assert!(
            std::fs::read_dir(current.path)
                .expect("failed project directory should remain inspectable")
                .next()
                .is_none()
        );
    }

    #[test]
    fn startup_marks_previously_completed_empty_projects_failed() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let database = home.path().join("builder.sqlite3");
        let mut store = Store::open(&database).expect("database should open");
        let session = store.create_session().expect("session should be created");
        let session = store
            .apply_planner_response(&session.id, session.revision, &project_plan_response())
            .expect("plan should be stored");

        let empty_directory = home.path().join("empty-project");
        std::fs::create_dir(&empty_directory).expect("empty project path should be created");
        let empty_project = store
            .create_project_record(
                &session.id,
                "empty-project",
                &empty_directory.to_string_lossy(),
            )
            .expect("empty project should be recorded");
        store
            .update_project_status(&empty_project.id, "completed", None)
            .expect("empty project should be marked completed for this regression test");

        let populated_directory = home.path().join("populated-project");
        std::fs::create_dir(&populated_directory)
            .expect("populated project path should be created");
        std::fs::write(populated_directory.join("main.rs"), "fn main() {}")
            .expect("project file should be written");
        let populated_project = store
            .create_project_record(
                &session.id,
                "populated-project",
                &populated_directory.to_string_lossy(),
            )
            .expect("populated project should be recorded");
        store
            .update_project_status(&populated_project.id, "completed", None)
            .expect("populated project should be marked completed");

        reconcile_project_records(&mut store).expect("startup recovery should succeed");

        assert_eq!(
            store
                .get_project(&empty_project.id)
                .expect("empty project should remain available")
                .status,
            "failed"
        );
        assert_eq!(
            store
                .get_project(&populated_project.id)
                .expect("populated project should remain available")
                .status,
            "completed"
        );
    }

    #[tokio::test]
    async fn existing_project_directory_is_not_overwritten() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let app = router(state_with_agent(
            fake_agent(project_plan_response()),
            home.path(),
        ));
        let session = create_planned_session(&app).await;
        let directory = home.path().join("sf/projects/existing");
        std::fs::create_dir_all(&directory).expect("existing project path should be created");
        std::fs::write(directory.join("sentinel.txt"), "keep").expect("sentinel should write");

        let response = request(
            app,
            "POST",
            &format!("/api/sessions/{}/projects", session.id),
            Some(json!({ "name": "existing" })),
        )
        .await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            std::fs::read_to_string(directory.join("sentinel.txt"))
                .expect("sentinel should remain"),
            "keep"
        );
    }

    #[tokio::test]
    async fn pinned_requirement_cannot_be_changed_by_agent_turn() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let fake = fake_agent(project_plan_response());
        let response_slot = Arc::clone(&fake.response);
        let app = router(state_with_agent(fake, home.path()));
        let session = create_planned_session(&app).await;
        let requirement = session.requirements[0].clone();

        let pinned = request(
            app.clone(),
            "PUT",
            &format!(
                "/api/sessions/{}/requirements/{}/pin",
                session.id, requirement.id
            ),
            None,
        )
        .await;
        assert_eq!(pinned.status(), StatusCode::OK);
        *response_slot
            .lock()
            .expect("fake response lock should work") = Some(PlannerResponse {
            schema_version: 1,
            message: "I changed it".to_owned(),
            architecture: None,
            requirement_changes: vec![RequirementChange::Update {
                id: requirement.id.clone(),
                text: "Change the project data".to_owned(),
            }],
            questions: vec![],
        });

        let response = request(
            app.clone(),
            "POST",
            &format!("/api/sessions/{}/messages", session.id),
            Some(json!({ "content": "Change it" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = request(
            app.clone(),
            "GET",
            &format!("/api/sessions/{}", session.id),
            None,
        )
        .await;
        let current: SessionState =
            serde_json::from_value(json_body(response).await).expect("session should be available");
        assert_eq!(current.requirements[0].text, requirement.text);
        assert!(current.requirements[0].pinned);

        let unpinned = request(
            app.clone(),
            "DELETE",
            &format!(
                "/api/sessions/{}/requirements/{}/pin",
                session.id, requirement.id
            ),
            None,
        )
        .await;
        assert_eq!(unpinned.status(), StatusCode::OK);
        *response_slot
            .lock()
            .expect("fake response lock should work") = Some(PlannerResponse {
            schema_version: 1,
            message: "The unlocked requirement was updated".to_owned(),
            architecture: None,
            requirement_changes: vec![RequirementChange::Update {
                id: requirement.id,
                text: "Change the project data after review".to_owned(),
            }],
            questions: vec![],
        });
        let response = request(
            app,
            "POST",
            &format!("/api/sessions/{}/messages", session.id),
            Some(json!({ "content": "Now change the unlocked requirement" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let current: SessionState = serde_json::from_value(json_body(response).await)
            .expect("updated session should deserialize");
        assert_eq!(
            current.requirements[0].text,
            "Change the project data after review"
        );
    }

    #[tokio::test]
    async fn deleted_requirements_are_restored_only_by_explicit_api_action() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let app = router(state_with_agent(
            fake_agent(project_plan_response()),
            home.path(),
        ));
        let session = create_planned_session(&app).await;
        let requirement_id = session.requirements[0].id.clone();

        let deleted = request(
            app.clone(),
            "DELETE",
            &format!("/api/sessions/{}/requirements/{requirement_id}", session.id),
            None,
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::OK);
        let deleted: SessionState = serde_json::from_value(json_body(deleted).await)
            .expect("deletion response should be a session");
        assert!(deleted.requirements.is_empty());
        assert_eq!(deleted.deleted_requirements[0].id, requirement_id);

        let restored = request(
            app,
            "POST",
            &format!(
                "/api/sessions/{}/deleted-requirements/{requirement_id}/restore",
                session.id
            ),
            None,
        )
        .await;
        assert_eq!(restored.status(), StatusCode::OK);
        let restored: SessionState = serde_json::from_value(json_body(restored).await)
            .expect("restore response should be a session");
        assert_eq!(restored.requirements.len(), 1);
        assert!(restored.deleted_requirements.is_empty());
    }

    #[tokio::test]
    async fn invalid_agent_schema_is_reported_without_applying_planning_changes() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let app = router(state_with_agent(
            fake_agent(PlannerResponse {
                schema_version: 99,
                message: "Unsupported output".to_owned(),
                architecture: Some(Architecture {
                    overview: "Must not be saved".to_owned(),
                    stack: vec![],
                    decisions: vec![],
                }),
                requirement_changes: vec![],
                questions: vec![],
            }),
            home.path(),
        ));
        let created = request(app.clone(), "POST", "/api/sessions", None).await;
        let session: SessionState = serde_json::from_value(json_body(created).await)
            .expect("created response should be a session");
        let response = request(
            app.clone(),
            "POST",
            &format!("/api/sessions/{}/messages", session.id),
            Some(json!({ "content": "Start planning" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let fetched = request(app, "GET", &format!("/api/sessions/{}", session.id), None).await;
        let current: SessionState =
            serde_json::from_value(json_body(fetched).await).expect("session should be available");
        assert!(current.architecture.is_none());
        assert!(current.requirements.is_empty());
        assert_eq!(current.messages.len(), 1);
    }

    #[tokio::test]
    async fn browser_cors_is_limited_to_loopback_origins() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let app = router(state_with_agent(
            fake_agent(project_plan_response()),
            home.path(),
        ));
        let local = Request::builder()
            .method("OPTIONS")
            .uri("/api/sessions")
            .header("origin", "http://localhost.:5173")
            .header("access-control-request-method", "PUT")
            .header("access-control-request-headers", "content-type")
            .body(Body::empty())
            .expect("preflight request should build");
        let response = app
            .clone()
            .oneshot(local)
            .await
            .expect("request should run");
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&axum::http::HeaderValue::from_static(
                "http://localhost.:5173"
            ))
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-headers")
                .is_some_and(|value| value.to_str().unwrap_or_default().contains("content-type"))
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-methods")
                .is_some_and(|value| value.to_str().unwrap_or_default().contains("PUT"))
        );

        let loopback_ip = Request::builder()
            .method("OPTIONS")
            .uri("/api/sessions")
            .header("origin", "http://127.0.0.1:8080")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .body(Body::empty())
            .expect("loopback IP preflight should build");
        let response = app
            .clone()
            .oneshot(loopback_ip)
            .await
            .expect("loopback preflight should run");
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&axum::http::HeaderValue::from_static(
                "http://127.0.0.1:8080"
            ))
        );

        let remote = Request::builder()
            .method("OPTIONS")
            .uri("/api/sessions")
            .header("origin", "https://example.com")
            .header("access-control-request-method", "POST")
            .body(Body::empty())
            .expect("preflight request should build");
        let response = app.oneshot(remote).await.expect("request should run");
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
    }
}
