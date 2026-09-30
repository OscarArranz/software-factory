use std::{path::Path, time::SystemTime};

use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::{
    DeletedRequirement, Message, MessageRole, PlannerResponse, Project, ProjectSnapshot,
    Requirement, RequirementChange, SessionState, ValidationError, new_id, normalize_requirement,
};

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    Conflict(String),
    Invalid(String),
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::Conflict(message) | Self::Invalid(message) => {
                formatter.write_str(message)
            }
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Serialization(error) => write!(formatter, "serialization error: {error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

impl From<ValidationError> for StoreError {
    fn from(error: ValidationError) -> Self {
        match error {
            ValidationError::Invalid(message) => Self::Invalid(message),
            ValidationError::Conflict(message) => Self::Conflict(message),
        }
    }
}

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let mut store = Self { connection };
        store.initialize()?;
        Ok(store)
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        let mut store = Self {
            connection: Connection::open_in_memory().expect("in-memory database should open"),
        };
        store
            .initialize()
            .expect("in-memory database schema should initialize");
        store
    }

    fn initialize(&mut self) -> Result<(), StoreError> {
        let schema_version = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
        if schema_version > 1 {
            return Err(StoreError::Invalid(format!(
                "database schema version {schema_version} is newer than this executable supports"
            )));
        }
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                revision INTEGER NOT NULL DEFAULT 0,
                architecture_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK(role IN ('user', 'assistant')),
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS messages_session_sequence
                ON messages(session_id, sequence);
            CREATE TABLE IF NOT EXISTS requirements (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                text TEXT NOT NULL,
                pinned INTEGER NOT NULL CHECK(pinned IN (0, 1)),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS requirements_session
                ON requirements(session_id);
            CREATE TABLE IF NOT EXISTS deleted_requirements (
                tombstone_id INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                text TEXT NOT NULL,
                deleted_at INTEGER NOT NULL,
                restored_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS deleted_requirements_session
                ON deleted_requirements(session_id, id, restored_at);
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL CHECK(status IN ('queued', 'running', 'completed', 'failed')),
                error TEXT,
                snapshot_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS projects_session
                ON projects(session_id, created_at);
            PRAGMA user_version = 1;",
        )?;
        Ok(())
    }

    pub fn create_session(&mut self) -> Result<SessionState, StoreError> {
        let id = new_id();
        let timestamp = now();
        self.connection.execute(
            "INSERT INTO sessions (id, revision, architecture_json, created_at, updated_at)
             VALUES (?1, 0, NULL, ?2, ?2)",
            params![id, timestamp],
        )?;
        self.get_session(&id)
    }

    pub fn get_session(&self, id: &str) -> Result<SessionState, StoreError> {
        load_session(&self.connection, id)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionState>, StoreError> {
        let ids = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM sessions ORDER BY updated_at DESC, id")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        ids.iter()
            .map(|id| self.get_session(id))
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn add_user_message(
        &mut self,
        session_id: &str,
        content: &str,
    ) -> Result<SessionState, StoreError> {
        let tx = self.connection.transaction()?;
        ensure_session(&tx, session_id)?;
        let timestamp = now();
        tx.execute(
            "INSERT INTO messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
            params![new_id(), session_id, content, timestamp],
        )?;
        bump_revision(&tx, session_id, timestamp)?;
        tx.commit()?;
        self.get_session(session_id)
    }

    pub fn apply_planner_response(
        &mut self,
        session_id: &str,
        expected_revision: i64,
        response: &PlannerResponse,
    ) -> Result<SessionState, StoreError> {
        let tx = self.connection.transaction()?;
        let state = load_session(&tx, session_id)?;
        if state.revision != expected_revision {
            return Err(StoreError::Conflict(
                "session changed while the planning agent was responding; reload and retry"
                    .to_owned(),
            ));
        }
        response.validate(&state)?;

        if let Some(architecture) = &response.architecture {
            tx.execute(
                "UPDATE sessions SET architecture_json = ?1 WHERE id = ?2",
                params![serde_json::to_string(architecture)?, session_id],
            )?;
        }

        let timestamp = now();
        for change in &response.requirement_changes {
            match change {
                RequirementChange::Add { text } => {
                    tx.execute(
                        "INSERT INTO requirements (id, session_id, text, pinned, created_at, updated_at)
                         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
                        params![new_id(), session_id, text.trim(), timestamp],
                    )?;
                }
                RequirementChange::Update { id, text } => {
                    let changed = tx.execute(
                        "UPDATE requirements SET text = ?1, updated_at = ?2
                         WHERE id = ?3 AND session_id = ?4 AND pinned = 0",
                        params![text.trim(), timestamp, id, session_id],
                    )?;
                    if changed != 1 {
                        return Err(StoreError::Conflict(format!(
                            "requirement `{id}` changed while the planning agent was responding"
                        )));
                    }
                }
            }
        }

        let content = if response.questions.is_empty() {
            response.message.clone()
        } else {
            format!(
                "{}\n\nQuestions:\n{}",
                response.message,
                response
                    .questions
                    .iter()
                    .map(|question| format!("- {question}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        tx.execute(
            "INSERT INTO messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, 'assistant', ?3, ?4)",
            params![new_id(), session_id, content, timestamp],
        )?;
        bump_revision(&tx, session_id, timestamp)?;
        tx.commit()?;
        self.get_session(session_id)
    }

    pub fn set_requirement_pinned(
        &mut self,
        session_id: &str,
        requirement_id: &str,
        pinned: bool,
    ) -> Result<SessionState, StoreError> {
        let tx = self.connection.transaction()?;
        let changed = tx.execute(
            "UPDATE requirements SET pinned = ?1, updated_at = ?2
             WHERE id = ?3 AND session_id = ?4",
            params![pinned, now(), requirement_id, session_id],
        )?;
        if changed == 0 {
            ensure_session(&tx, session_id)?;
            return Err(StoreError::NotFound(format!(
                "requirement `{requirement_id}` was not found"
            )));
        }
        bump_revision(&tx, session_id, now())?;
        tx.commit()?;
        self.get_session(session_id)
    }

    pub fn delete_requirement(
        &mut self,
        session_id: &str,
        requirement_id: &str,
    ) -> Result<SessionState, StoreError> {
        let tx = self.connection.transaction()?;
        let text = tx
            .query_row(
                "SELECT text FROM requirements WHERE id = ?1 AND session_id = ?2",
                params![requirement_id, session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::NotFound(format!("requirement `{requirement_id}` was not found"))
            })?;
        let timestamp = now();
        tx.execute(
            "DELETE FROM requirements WHERE id = ?1 AND session_id = ?2",
            params![requirement_id, session_id],
        )?;
        tx.execute(
            "INSERT INTO deleted_requirements (id, session_id, text, deleted_at, restored_at)
             VALUES (?1, ?2, ?3, ?4, NULL)",
            params![requirement_id, session_id, text, timestamp],
        )?;
        bump_revision(&tx, session_id, timestamp)?;
        tx.commit()?;
        self.get_session(session_id)
    }

    pub fn restore_requirement(
        &mut self,
        session_id: &str,
        requirement_id: &str,
    ) -> Result<SessionState, StoreError> {
        let tx = self.connection.transaction()?;
        let text = tx
            .query_row(
                "SELECT text FROM deleted_requirements
                 WHERE id = ?1 AND session_id = ?2 AND restored_at IS NULL",
                params![requirement_id, session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "deleted requirement `{requirement_id}` was not found"
                ))
            })?;
        let existing_texts = {
            let mut statement =
                tx.prepare("SELECT text FROM requirements WHERE session_id = ?1")?;
            statement
                .query_map([session_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        if existing_texts
            .iter()
            .any(|existing| normalize_requirement(existing) == normalize_requirement(&text))
        {
            return Err(StoreError::Conflict(
                "an equivalent active requirement already exists".to_owned(),
            ));
        }

        let timestamp = now();
        tx.execute(
            "INSERT INTO requirements (id, session_id, text, pinned, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?4)",
            params![requirement_id, session_id, text, timestamp],
        )?;
        tx.execute(
            "UPDATE deleted_requirements SET restored_at = ?1
             WHERE tombstone_id = (
                 SELECT tombstone_id FROM deleted_requirements
                 WHERE id = ?2 AND session_id = ?3 AND restored_at IS NULL
                 ORDER BY deleted_at DESC LIMIT 1
             )",
            params![timestamp, requirement_id, session_id],
        )?;
        bump_revision(&tx, session_id, timestamp)?;
        tx.commit()?;
        self.get_session(session_id)
    }

    pub fn create_project_record(
        &mut self,
        session_id: &str,
        name: &str,
        path: &str,
    ) -> Result<Project, StoreError> {
        let state = self.get_session(session_id)?;
        let architecture = state.architecture.ok_or_else(|| {
            StoreError::Invalid("project architecture has not been agreed".to_owned())
        })?;
        if state.requirements.is_empty() {
            return Err(StoreError::Invalid(
                "at least one active requirement is needed to create a project".to_owned(),
            ));
        }
        let snapshot = ProjectSnapshot {
            architecture,
            requirements: state.requirements,
        };
        let project = Project {
            id: new_id(),
            session_id: session_id.to_owned(),
            name: name.to_owned(),
            path: path.to_owned(),
            status: "queued".to_owned(),
            error: None,
            snapshot,
            created_at: now(),
            updated_at: now(),
        };
        self.connection.execute(
            "INSERT INTO projects
             (id, session_id, name, path, status, error, snapshot_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?7)",
            params![
                project.id,
                project.session_id,
                project.name,
                project.path,
                project.status,
                serde_json::to_string(&project.snapshot)?,
                project.created_at
            ],
        )?;
        Ok(project)
    }

    pub fn update_project_status(
        &mut self,
        project_id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<Project, StoreError> {
        let changed = self.connection.execute(
            "UPDATE projects SET status = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
            params![status, error, now(), project_id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "project `{project_id}` was not found"
            )));
        }
        self.get_project(project_id)
    }

    pub fn get_project(&self, id: &str) -> Result<Project, StoreError> {
        self.connection
            .query_row(
                "SELECT id, session_id, name, path, status, error, snapshot_json, created_at, updated_at
                 FROM projects WHERE id = ?1",
                [id],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("project `{id}` was not found")))
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id, session_id, name, path, status, error, snapshot_json, created_at, updated_at
             FROM projects ORDER BY created_at DESC",
        )?;
        statement
            .query_map([], project_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::Database)
    }
}

fn load_session(connection: &Connection, id: &str) -> Result<SessionState, StoreError> {
    let row = connection
        .query_row(
            "SELECT id, revision, architecture_json, created_at, updated_at
             FROM sessions WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("session `{id}` was not found")))?;

    let messages = {
        let mut statement = connection.prepare(
            "SELECT id, role, content, created_at FROM messages
             WHERE session_id = ?1 ORDER BY sequence",
        )?;
        statement
            .query_map([id], |row| {
                let role: String = row.get(1)?;
                Ok(Message {
                    id: row.get(0)?,
                    role: match role.as_str() {
                        "user" => MessageRole::User,
                        _ => MessageRole::Assistant,
                    },
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let requirements = {
        let mut statement = connection.prepare(
            "SELECT id, text, pinned FROM requirements WHERE session_id = ?1 ORDER BY created_at, id",
        )?;
        statement
            .query_map([id], |row| {
                Ok(Requirement {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    pinned: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let deleted_requirements = {
        let mut statement = connection.prepare(
            "SELECT id, text, deleted_at FROM deleted_requirements
             WHERE session_id = ?1 AND restored_at IS NULL ORDER BY deleted_at, id",
        )?;
        statement
            .query_map([id], |row| {
                Ok(DeletedRequirement {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    deleted_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(SessionState {
        id: row.0,
        revision: row.1,
        architecture: row.2.map(|json| serde_json::from_str(&json)).transpose()?,
        messages,
        requirements,
        deleted_requirements,
        created_at: row.3,
        updated_at: row.4,
    })
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let snapshot_json: String = row.get(6)?;
    let snapshot = serde_json::from_str(&snapshot_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(Project {
        id: row.get(0)?,
        session_id: row.get(1)?,
        name: row.get(2)?,
        path: row.get(3)?,
        status: row.get(4)?,
        error: row.get(5)?,
        snapshot,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn ensure_session(connection: &Connection, session_id: &str) -> Result<(), StoreError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
        [session_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(StoreError::NotFound(format!(
            "session `{session_id}` was not found"
        )));
    }
    Ok(())
}

fn bump_revision(
    connection: &Connection,
    session_id: &str,
    timestamp: i64,
) -> Result<(), StoreError> {
    let changed = connection.execute(
        "UPDATE sessions SET revision = revision + 1, updated_at = ?1 WHERE id = ?2",
        params![timestamp, session_id],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(format!(
            "session `{session_id}` was not found"
        )));
    }
    Ok(())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::{Store, StoreError};
    use crate::domain::{Architecture, PlannerResponse, RequirementChange};

    #[test]
    fn persists_sessions_and_deleted_requirement_tombstones() {
        let mut store = Store::in_memory();
        let session = store.create_session().expect("session should be created");
        let response = PlannerResponse {
            schema_version: 1,
            message: "A requirement was added".to_owned(),
            architecture: Some(Architecture {
                overview: "A small service".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Store data locally".to_owned(),
            }],
            questions: vec![],
        };
        let session = store
            .apply_planner_response(&session.id, session.revision, &response)
            .expect("agent response should be stored");
        let requirement_id = session.requirements[0].id.clone();

        let session = store
            .delete_requirement(&session.id, &requirement_id)
            .expect("requirement should be deleted");
        let loaded = store
            .get_session(&session.id)
            .expect("session should reload from SQLite");

        assert!(loaded.requirements.is_empty());
        assert_eq!(loaded.deleted_requirements.len(), 1);
        assert_eq!(loaded.deleted_requirements[0].text, "Store data locally");
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn agent_turn_is_rejected_if_session_changed_after_snapshot() {
        let mut store = Store::in_memory();
        let session = store.create_session().expect("session should be created");
        let state = store
            .add_user_message(&session.id, "Build an app")
            .expect("user message should be stored");
        store
            .add_user_message(&session.id, "Another message")
            .expect("another message should be stored");

        let response = PlannerResponse {
            schema_version: 1,
            message: "Response".to_owned(),
            architecture: None,
            requirement_changes: vec![],
            questions: vec![],
        };
        assert!(matches!(
            store.apply_planner_response(&session.id, state.revision, &response),
            Err(StoreError::Conflict(_))
        ));
    }

    #[test]
    fn restores_deleted_requirement_only_explicitly() {
        let mut store = Store::in_memory();
        let session = store.create_session().expect("session should be created");
        let response = PlannerResponse {
            schema_version: 1,
            message: "A requirement was added".to_owned(),
            architecture: Some(Architecture {
                overview: "A service".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Persist projects".to_owned(),
            }],
            questions: vec![],
        };
        let session = store
            .apply_planner_response(&session.id, session.revision, &response)
            .expect("agent response should be stored");
        let id = session.requirements[0].id.clone();
        store
            .delete_requirement(&session.id, &id)
            .expect("requirement should be deleted");
        let restored = store
            .restore_requirement(&session.id, &id)
            .expect("explicit restore should succeed");

        assert_eq!(restored.requirements.len(), 1);
        assert!(restored.deleted_requirements.is_empty());
    }

    #[test]
    fn deleted_requirement_cannot_be_recreated_by_agent() {
        let mut store = Store::in_memory();
        let session = store.create_session().expect("session should be created");
        let response = PlannerResponse {
            schema_version: 1,
            message: "A requirement was added".to_owned(),
            architecture: Some(Architecture {
                overview: "A service".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Persist project data".to_owned(),
            }],
            questions: vec![],
        };
        let session = store
            .apply_planner_response(&session.id, session.revision, &response)
            .expect("agent response should be stored");
        let id = session.requirements[0].id.clone();
        let session = store
            .delete_requirement(&session.id, &id)
            .expect("requirement should be deleted");
        let response = PlannerResponse {
            schema_version: 1,
            message: "I added that requirement again".to_owned(),
            architecture: None,
            requirement_changes: vec![RequirementChange::Add {
                text: "  PERSIST project data ".to_owned(),
            }],
            questions: vec![],
        };

        assert!(matches!(
            store.apply_planner_response(&session.id, session.revision, &response),
            Err(StoreError::Conflict(_))
        ));
        assert!(
            store
                .get_session(&session.id)
                .expect("session should remain available")
                .requirements
                .is_empty()
        );

        let update = PlannerResponse {
            schema_version: 1,
            message: "I updated the deleted requirement".to_owned(),
            architecture: None,
            requirement_changes: vec![RequirementChange::Update {
                id,
                text: "Recreate the deleted requirement".to_owned(),
            }],
            questions: vec![],
        };
        assert!(
            store
                .apply_planner_response(&session.id, session.revision, &update)
                .is_err()
        );
    }

    #[test]
    fn a_restored_requirement_can_be_deleted_again() {
        let mut store = Store::in_memory();
        let session = store.create_session().expect("session should be created");
        let response = PlannerResponse {
            schema_version: 1,
            message: "A requirement was added".to_owned(),
            architecture: Some(Architecture {
                overview: "A service".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Persist projects".to_owned(),
            }],
            questions: vec![],
        };
        let session = store
            .apply_planner_response(&session.id, session.revision, &response)
            .expect("agent response should be stored");
        let id = session.requirements[0].id.clone();
        store
            .delete_requirement(&session.id, &id)
            .expect("first deletion should work");
        store
            .restore_requirement(&session.id, &id)
            .expect("explicit restore should succeed");
        let deleted_again = store
            .delete_requirement(&session.id, &id)
            .expect("second deletion should work");

        assert!(deleted_again.requirements.is_empty());
        assert_eq!(deleted_again.deleted_requirements.len(), 1);
    }

    #[test]
    fn sessions_survive_reopening_the_sqlite_database() {
        let directory = tempfile::tempdir().expect("temp dir should be created");
        let path = directory.path().join("builder.sqlite3");
        let (session_id, project_id) = {
            let mut store = Store::open(&path).expect("database should open");
            let session = store.create_session().expect("session should be created");
            let session = store
                .add_user_message(&session.id, "Build a small app")
                .expect("user message should be stored");
            let response = PlannerResponse {
                schema_version: 1,
                message: "A plan was agreed".to_owned(),
                architecture: Some(Architecture {
                    overview: "A persisted architecture".to_owned(),
                    stack: vec![],
                    decisions: vec![],
                }),
                requirement_changes: vec![RequirementChange::Add {
                    text: "Persist its data".to_owned(),
                }],
                questions: vec![],
            };
            let session = store
                .apply_planner_response(&session.id, session.revision, &response)
                .expect("agent response should be stored");
            let project = store
                .create_project_record(&session.id, "demo", "/tmp/demo")
                .expect("project record should be stored");
            store
                .update_project_status(&project.id, "completed", None)
                .expect("project status should be stored");
            store
                .delete_requirement(&session.id, &session.requirements[0].id)
                .expect("tombstone should be stored");
            (session.id, project.id)
        };
        let store = Store::open(&path).expect("database should reopen");

        let session = store
            .get_session(&session_id)
            .expect("session should persist");
        assert_eq!(session.messages.len(), 2);
        assert_eq!(
            session
                .architecture
                .expect("architecture should persist")
                .overview,
            "A persisted architecture"
        );
        assert!(session.requirements.is_empty());
        assert_eq!(session.deleted_requirements[0].text, "Persist its data");
        let project = store
            .get_project(&project_id)
            .expect("project should persist");
        assert_eq!(project.status, "completed");
        assert_eq!(project.snapshot.requirements[0].text, "Persist its data");
    }

    #[test]
    fn rejected_agent_turn_does_not_partially_update_architecture() {
        let mut store = Store::in_memory();
        let session = store.create_session().expect("session should be created");
        let initial = PlannerResponse {
            schema_version: 1,
            message: "Initial plan".to_owned(),
            architecture: Some(Architecture {
                overview: "Original architecture".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Add {
                text: "Keep the data local".to_owned(),
            }],
            questions: vec![],
        };
        let session = store
            .apply_planner_response(&session.id, session.revision, &initial)
            .expect("initial plan should be stored");
        let requirement_id = session.requirements[0].id.clone();
        let session = store
            .set_requirement_pinned(&session.id, &requirement_id, true)
            .expect("requirement should be pinned");
        let session = store
            .add_user_message(&session.id, "Continue planning")
            .expect("user message should be stored");
        let invalid = PlannerResponse {
            schema_version: 1,
            message: "A conflicting update".to_owned(),
            architecture: Some(Architecture {
                overview: "Changed architecture".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            requirement_changes: vec![RequirementChange::Update {
                id: requirement_id.clone(),
                text: "Change the pinned requirement".to_owned(),
            }],
            questions: vec![],
        };

        assert!(
            store
                .apply_planner_response(&session.id, session.revision, &invalid)
                .is_err()
        );
        let current = store
            .get_session(&session.id)
            .expect("session should still be available");
        assert_eq!(
            current
                .architecture
                .expect("architecture should remain")
                .overview,
            "Original architecture"
        );
        assert_eq!(current.requirements[0].text, "Keep the data local");
    }
}
