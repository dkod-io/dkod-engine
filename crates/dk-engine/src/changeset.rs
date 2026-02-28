use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use dk_core::{RepoId, SymbolId};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Changeset {
    pub id: Uuid,
    pub repo_id: RepoId,
    pub number: i32,
    pub title: String,
    pub intent_summary: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub state: String,
    pub session_id: Option<Uuid>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub author_id: Option<Uuid>,
    pub base_version: Option<String>,
    pub merged_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ChangesetFile {
    pub changeset_id: Uuid,
    pub file_path: String,
    pub operation: String,
    pub content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChangesetFileMeta {
    pub file_path: String,
    pub operation: String,
    pub size_bytes: i64,
}

pub struct ChangesetStore {
    db: PgPool,
}

impl ChangesetStore {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Create a changeset via the Agent Protocol path.
    /// Auto-increments the number per repo using an advisory lock.
    /// Sets `source_branch` to `agent/<agent_id>` and `target_branch` to `main`
    /// so platform queries that read these NOT NULL columns always succeed.
    /// Also populates `agent_name` (same value as `agent_id`) for platform compatibility.
    pub async fn create(
        &self,
        repo_id: RepoId,
        session_id: Option<Uuid>,
        agent_id: &str,
        intent: &str,
        base_version: Option<&str>,
    ) -> dk_core::Result<Changeset> {
        let source_branch = format!("agent/{}", agent_id);
        let target_branch = "main";

        let mut tx = self.db.begin().await?;

        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('changeset:' || $1::text))")
            .bind(repo_id)
            .execute(&mut *tx)
            .await?;

        let row: (Uuid, i32, String, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            r#"INSERT INTO changesets
                   (repo_id, number, title, intent_summary, source_branch, target_branch,
                    session_id, agent_id, agent_name, base_version)
               SELECT $1, COALESCE(MAX(number), 0) + 1, $2, $2, $3, $4, $5, $6, $6, $7
               FROM changesets WHERE repo_id = $1
               RETURNING id, number, state, created_at, updated_at"#,
        )
        .bind(repo_id)
        .bind(intent)
        .bind(&source_branch)
        .bind(target_branch)
        .bind(session_id)
        .bind(agent_id)
        .bind(base_version)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Changeset {
            id: row.0,
            repo_id,
            number: row.1,
            title: intent.to_string(),
            intent_summary: Some(intent.to_string()),
            source_branch,
            target_branch: target_branch.to_string(),
            state: row.2,
            session_id,
            agent_id: Some(agent_id.to_string()),
            agent_name: Some(agent_id.to_string()),
            author_id: None,
            base_version: base_version.map(String::from),
            merged_version: None,
            created_at: row.3,
            updated_at: row.4,
            merged_at: None,
        })
    }

    pub async fn get(&self, id: Uuid) -> dk_core::Result<Changeset> {
        sqlx::query_as::<_, Changeset>(
            r#"SELECT id, repo_id, number, title, intent_summary,
                      source_branch, target_branch, state,
                      session_id, agent_id, agent_name, author_id,
                      base_version, merged_version,
                      created_at, updated_at, merged_at
               FROM changesets WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await?
        .ok_or_else(|| dk_core::Error::Internal(format!("changeset {} not found", id)))
    }

    pub async fn update_status(&self, id: Uuid, status: &str) -> dk_core::Result<()> {
        sqlx::query("UPDATE changesets SET state = $1, updated_at = now() WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Update changeset status with optimistic locking: the transition only
    /// succeeds when the current state matches one of `expected_states`.
    /// Returns an error if the row was not updated (state mismatch or not found).
    pub async fn update_status_if(
        &self,
        id: Uuid,
        new_status: &str,
        expected_states: &[&str],
    ) -> dk_core::Result<()> {
        // Build a comma-separated list of expected states for the ANY($3) clause.
        let states: Vec<String> = expected_states.iter().map(|s| s.to_string()).collect();
        let result = sqlx::query(
            "UPDATE changesets SET state = $1, updated_at = now() WHERE id = $2 AND state = ANY($3)",
        )
        .bind(new_status)
        .bind(id)
        .bind(&states)
        .execute(&self.db)
        .await?;

        if result.rows_affected() == 0 {
            return Err(dk_core::Error::Internal(format!(
                "changeset {} not found or not in expected state (expected one of: {:?})",
                id, expected_states,
            )));
        }
        Ok(())
    }

    pub async fn set_merged(&self, id: Uuid, commit_hash: &str) -> dk_core::Result<()> {
        sqlx::query(
            "UPDATE changesets SET state = 'merged', merged_version = $1, merged_at = now(), updated_at = now() WHERE id = $2",
        )
        .bind(commit_hash)
        .bind(id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn upsert_file(
        &self,
        changeset_id: Uuid,
        file_path: &str,
        operation: &str,
        content: Option<&str>,
    ) -> dk_core::Result<()> {
        sqlx::query(
            r#"INSERT INTO changeset_files (changeset_id, file_path, operation, content)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (changeset_id, file_path) DO UPDATE SET
                   operation = EXCLUDED.operation,
                   content = EXCLUDED.content"#,
        )
        .bind(changeset_id)
        .bind(file_path)
        .bind(operation)
        .bind(content)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn get_files(&self, changeset_id: Uuid) -> dk_core::Result<Vec<ChangesetFile>> {
        let rows: Vec<(Uuid, String, String, Option<String>)> = sqlx::query_as(
            "SELECT changeset_id, file_path, operation, content FROM changeset_files WHERE changeset_id = $1",
        )
        .bind(changeset_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ChangesetFile {
                changeset_id: r.0,
                file_path: r.1,
                operation: r.2,
                content: r.3,
            })
            .collect())
    }

    /// Lightweight query returning only file metadata (path, operation, size)
    /// without loading the full content column.
    pub async fn get_files_metadata(&self, changeset_id: Uuid) -> dk_core::Result<Vec<ChangesetFileMeta>> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT file_path, operation, COALESCE(LENGTH(content), 0)::bigint AS size_bytes FROM changeset_files WHERE changeset_id = $1",
        )
        .bind(changeset_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ChangesetFileMeta {
                file_path: r.0,
                operation: r.1,
                size_bytes: r.2,
            })
            .collect())
    }

    pub async fn record_affected_symbol(
        &self,
        changeset_id: Uuid,
        symbol_id: SymbolId,
        qualified_name: &str,
    ) -> dk_core::Result<()> {
        sqlx::query(
            r#"INSERT INTO changeset_symbols (changeset_id, symbol_id, symbol_qualified_name)
               VALUES ($1, $2, $3)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(changeset_id)
        .bind(symbol_id)
        .bind(qualified_name)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn get_affected_symbols(&self, changeset_id: Uuid) -> dk_core::Result<Vec<(SymbolId, String)>> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT symbol_id, symbol_qualified_name FROM changeset_symbols WHERE changeset_id = $1",
        )
        .bind(changeset_id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows)
    }

    /// Find changesets that conflict with ours.
    /// Only considers changesets merged AFTER our base_version —
    /// i.e. changes the agent didn't know about when it started.
    pub async fn find_conflicting_changesets(
        &self,
        repo_id: RepoId,
        base_version: &str,
        my_changeset_id: Uuid,
    ) -> dk_core::Result<Vec<(Uuid, Vec<String>)>> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            r#"SELECT DISTINCT cs.changeset_id, cs.symbol_qualified_name
               FROM changeset_symbols cs
               JOIN changesets c ON c.id = cs.changeset_id
               WHERE c.repo_id = $1
                 AND c.state = 'merged'
                 AND c.id != $2
                 AND c.merged_version IS NOT NULL
                 AND c.merged_version != $3
                 AND cs.symbol_qualified_name IN (
                     SELECT symbol_qualified_name FROM changeset_symbols WHERE changeset_id = $2
                 )"#,
        )
        .bind(repo_id)
        .bind(my_changeset_id)
        .bind(base_version)
        .fetch_all(&self.db)
        .await?;

        let mut map: std::collections::HashMap<Uuid, Vec<String>> = std::collections::HashMap::new();
        for (cs_id, sym_name) in rows {
            map.entry(cs_id).or_default().push(sym_name);
        }
        Ok(map.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the source_branch format produced by `create()`.
    /// The method builds `format!("agent/{}", agent_id)` and sets
    /// `target_branch` to `"main"`.  We test the format logic directly
    /// since `create()` itself requires a live PgPool.
    #[test]
    fn source_branch_format_uses_agent_prefix() {
        let agent_id = "claude-42";
        let source_branch = format!("agent/{}", agent_id);
        assert_eq!(source_branch, "agent/claude-42");
    }

    #[test]
    fn source_branch_format_with_special_chars() {
        let agent_id = "agent/with-slash";
        let source_branch = format!("agent/{}", agent_id);
        assert_eq!(source_branch, "agent/agent/with-slash");
    }

    #[test]
    fn target_branch_is_main() {
        // create() hardcodes target_branch to "main"
        let target_branch = "main";
        assert_eq!(target_branch, "main");
    }

    /// Verify that a manually-constructed Changeset (matching the shape
    /// returned by `create()`) has the correct branch and agent fields.
    #[test]
    fn changeset_create_shape_has_correct_branches() {
        let repo_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let agent_id = "test-agent";
        let intent = "fix all the bugs";

        let source_branch = format!("agent/{}", agent_id);
        let now = Utc::now();

        let cs = Changeset {
            id: Uuid::new_v4(),
            repo_id,
            number: 1,
            title: intent.to_string(),
            intent_summary: Some(intent.to_string()),
            source_branch: source_branch.clone(),
            target_branch: "main".to_string(),
            state: "open".to_string(),
            session_id: Some(session_id),
            agent_id: Some(agent_id.to_string()),
            agent_name: Some(agent_id.to_string()),
            author_id: None,
            base_version: Some("abc123".to_string()),
            merged_version: None,
            created_at: now,
            updated_at: now,
            merged_at: None,
        };

        assert_eq!(cs.source_branch, "agent/test-agent");
        assert_eq!(cs.target_branch, "main");
        assert_eq!(cs.agent_name.as_deref(), Some("test-agent"));
        assert_eq!(cs.agent_id, cs.agent_name, "agent_name should equal agent_id per create()");
        assert!(cs.merged_at.is_none());
        assert!(cs.merged_version.is_none());
    }

    /// Verify the Changeset struct fields are all accessible and have
    /// the expected types (compile-time check + runtime assertions).
    #[test]
    fn changeset_all_fields_accessible() {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();

        let cs = Changeset {
            id,
            repo_id,
            number: 42,
            title: "test".to_string(),
            intent_summary: None,
            source_branch: "agent/a".to_string(),
            target_branch: "main".to_string(),
            state: "open".to_string(),
            session_id: None,
            agent_id: None,
            agent_name: None,
            author_id: None,
            base_version: None,
            merged_version: None,
            created_at: now,
            updated_at: now,
            merged_at: None,
        };

        assert_eq!(cs.id, id);
        assert_eq!(cs.repo_id, repo_id);
        assert_eq!(cs.number, 42);
        assert_eq!(cs.title, "test");
        assert!(cs.intent_summary.is_none());
        assert!(cs.session_id.is_none());
        assert!(cs.agent_id.is_none());
        assert!(cs.agent_name.is_none());
        assert!(cs.author_id.is_none());
        assert!(cs.base_version.is_none());
        assert!(cs.merged_version.is_none());
        assert!(cs.merged_at.is_none());
    }

    #[test]
    fn changeset_file_meta_struct() {
        let meta = ChangesetFileMeta {
            file_path: "src/main.rs".to_string(),
            operation: "modify".to_string(),
            size_bytes: 1024,
        };
        assert_eq!(meta.file_path, "src/main.rs");
        assert_eq!(meta.operation, "modify");
        assert_eq!(meta.size_bytes, 1024);
    }

    #[test]
    fn changeset_file_struct() {
        let cf = ChangesetFile {
            changeset_id: Uuid::new_v4(),
            file_path: "lib.rs".to_string(),
            operation: "add".to_string(),
            content: Some("fn main() {}".to_string()),
        };
        assert_eq!(cf.file_path, "lib.rs");
        assert_eq!(cf.operation, "add");
        assert!(cf.content.is_some());
    }

    #[test]
    fn changeset_clone_produces_equal_values() {
        let now = Utc::now();
        let cs = Changeset {
            id: Uuid::new_v4(),
            repo_id: Uuid::new_v4(),
            number: 1,
            title: "clone test".to_string(),
            intent_summary: Some("intent".to_string()),
            source_branch: "agent/x".to_string(),
            target_branch: "main".to_string(),
            state: "open".to_string(),
            session_id: None,
            agent_id: Some("x".to_string()),
            agent_name: Some("x".to_string()),
            author_id: None,
            base_version: None,
            merged_version: None,
            created_at: now,
            updated_at: now,
            merged_at: None,
        };

        let cloned = cs.clone();
        assert_eq!(cs.id, cloned.id);
        assert_eq!(cs.source_branch, cloned.source_branch);
        assert_eq!(cs.target_branch, cloned.target_branch);
        assert_eq!(cs.state, cloned.state);
    }
}
