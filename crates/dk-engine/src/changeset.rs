use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use dk_core::{RepoId, SymbolId};

/// Explicit changeset states. Replaces the former ambiguous "open" state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangesetState {
    Draft,
    Submitted,
    Verifying,
    Approved,
    Rejected,
    Merged,
    Closed,
}

impl ChangesetState {
    /// Parse a state string from the database into a `ChangesetState`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "submitted" => Some(Self::Submitted),
            "verifying" => Some(Self::Verifying),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "merged" => Some(Self::Merged),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }

    /// Return the database string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Submitted => "submitted",
            Self::Verifying => "verifying",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }

    /// Check whether transitioning from `self` to `target` is valid.
    ///
    /// Valid transitions:
    /// - draft      -> submitted
    /// - submitted  -> verifying
    /// - verifying  -> approved | rejected
    /// - approved   -> merged
    /// - any        -> closed
    pub fn can_transition_to(&self, target: Self) -> bool {
        if target == Self::Closed {
            return true;
        }
        matches!(
            (self, target),
            (Self::Draft, Self::Submitted)
                | (Self::Submitted, Self::Verifying)
                | (Self::Verifying, Self::Approved)
                | (Self::Verifying, Self::Rejected)
                | (Self::Approved, Self::Merged)
        )
    }

    /// True when the changeset is in a terminal state and its backing
    /// workspace no longer needs to be preserved.
    ///
    /// `Draft` is considered terminal because it represents a pre-submit
    /// workspace that was never progressed — there is no protected in-flight
    /// state worth pinning.
    ///
    /// Used by the workspace pin guard (Epic B) to decide
    /// whether to evict or skip a candidate workspace.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Draft | Self::Merged | Self::Rejected | Self::Closed)
    }
}

impl std::fmt::Display for ChangesetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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
    pub reason: String,
    pub session_id: Option<Uuid>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub author_id: Option<Uuid>,
    pub base_version: Option<String>,
    pub merged_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
    /// Stacked-changeset parent. PR1 ships the column additively — nothing
    /// populates it and no consumer reads it. PR2 will set it at submit
    /// time and enforce merge-order on the chain.
    pub parent_changeset_id: Option<Uuid>,
}

impl Changeset {
    /// Parse the current state string into a typed `ChangesetState`.
    pub fn parsed_state(&self) -> Option<ChangesetState> {
        ChangesetState::parse(&self.state)
    }

    /// Validate and perform a state transition, recording the reason.
    /// Returns an error if the transition is not allowed.
    pub fn transition(
        &mut self,
        target: ChangesetState,
        reason: impl Into<String>,
    ) -> dk_core::Result<()> {
        let current = self.parsed_state().ok_or_else(|| {
            dk_core::Error::Internal(format!("unknown current state: '{}'", self.state))
        })?;

        if !current.can_transition_to(target) {
            return Err(dk_core::Error::InvalidInput(format!(
                "invalid state transition: '{}' -> '{}'",
                current, target,
            )));
        }

        self.state = target.as_str().to_string();
        self.reason = reason.into();
        Ok(())
    }
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
    /// Sets `source_branch` to `agent/<agent_name>` and `target_branch` to `main`
    /// so platform queries that read these NOT NULL columns always succeed.
    /// `agent_name` is the human-readable name (e.g. "agent-1" or "feature-bot").
    pub async fn create(
        &self,
        repo_id: RepoId,
        session_id: Option<Uuid>,
        agent_id: &str,
        intent: &str,
        base_version: Option<&str>,
        agent_name: &str,
    ) -> dk_core::Result<Changeset> {
        let intent_slug: String = intent
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        let slug = if intent_slug.len() > 50 {
            let cut = intent_slug
                .char_indices()
                .take_while(|(i, _)| *i < 50)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            intent_slug[..cut].trim_end_matches('-').to_string()
        } else {
            intent_slug
        };
        let source_branch = format!("{}/{}", slug, agent_name);
        let target_branch = "main";

        let mut tx = self.db.begin().await?;

        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('changeset:' || $1::text))")
            .bind(repo_id)
            .execute(&mut *tx)
            .await?;

        let row: (Uuid, i32, String, String, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            r#"INSERT INTO changesets
                   (repo_id, number, title, intent_summary, source_branch, target_branch,
                    state, reason, session_id, agent_id, agent_name, base_version)
               SELECT $1, COALESCE(MAX(number), 0) + 1, $2, $2, $3, $4,
                    'draft', 'created via agent connect', $5, $6, $7, $8
               FROM changesets WHERE repo_id = $1
               RETURNING id, number, state, reason, created_at, updated_at"#,
        )
        .bind(repo_id)
        .bind(intent)
        .bind(&source_branch)
        .bind(target_branch)
        .bind(session_id)
        .bind(agent_id)
        .bind(agent_name)
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
            reason: row.3,
            session_id,
            agent_id: Some(agent_id.to_string()),
            agent_name: Some(agent_name.to_string()),
            author_id: None,
            base_version: base_version.map(String::from),
            merged_version: None,
            created_at: row.4,
            updated_at: row.5,
            merged_at: None,
            parent_changeset_id: None,
        })
    }

    pub async fn get(&self, id: Uuid) -> dk_core::Result<Changeset> {
        sqlx::query_as::<_, Changeset>(
            r#"SELECT id, repo_id, number, title, intent_summary,
                      source_branch, target_branch, state, reason,
                      session_id, agent_id, agent_name, author_id,
                      base_version, merged_version,
                      created_at, updated_at, merged_at,
                      parent_changeset_id
               FROM changesets WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await?
        .ok_or_else(|| dk_core::Error::Internal(format!("changeset {} not found", id)))
    }

    pub async fn update_status(&self, id: Uuid, status: &str) -> dk_core::Result<()> {
        self.update_status_with_reason(id, status, "").await
    }

    /// Update changeset status and record the reason for the transition.
    pub async fn update_status_with_reason(
        &self,
        id: Uuid,
        status: &str,
        reason: &str,
    ) -> dk_core::Result<()> {
        sqlx::query(
            "UPDATE changesets SET state = $1, reason = $2, updated_at = now() WHERE id = $3",
        )
        .bind(status)
        .bind(reason)
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
        self.update_status_if_with_reason(id, new_status, expected_states, "").await
    }

    /// Like `update_status_if` but also records a reason for the transition.
    pub async fn update_status_if_with_reason(
        &self,
        id: Uuid,
        new_status: &str,
        expected_states: &[&str],
        reason: &str,
    ) -> dk_core::Result<()> {
        let states: Vec<String> = expected_states.iter().map(|s| s.to_string()).collect();
        let result = sqlx::query(
            "UPDATE changesets SET state = $1, reason = $2, updated_at = now() WHERE id = $3 AND state = ANY($4)",
        )
        .bind(new_status)
        .bind(reason)
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
            "UPDATE changesets SET state = 'merged', reason = 'merge completed', merged_version = $1, merged_at = now(), updated_at = now() WHERE id = $2",
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

    /// List live competitors on a file path.
    ///
    /// Returns every changeset in `{submitted, verifying, approved}` for
    /// `repo_id` that has a `changeset_files` row for `path`. `draft`,
    /// `merged`, `rejected`, and `closed` are filtered at the SQL level —
    /// `draft` is session-local, `merged` is handled by the AST merger at
    /// `dk_merge`, and the rest are inert. Policy (self-exclusion, stale
    /// comparison vs. session read timestamp) is applied by the pure
    /// `dk_protocol::stale_overlay::is_stale` helper.
    ///
    /// Used by the STALE_OVERLAY pre-write check in `handle_file_write`.
    pub async fn list_path_competitors(
        &self,
        repo_id: RepoId,
        path: &str,
    ) -> dk_core::Result<Vec<(Uuid, Option<Uuid>, String, DateTime<Utc>)>> {
        let rows: Vec<(Uuid, Option<Uuid>, String, DateTime<Utc>)> = sqlx::query_as(
            r#"SELECT c.id, c.session_id, c.state, c.updated_at
               FROM changesets c
               JOIN changeset_files cf ON cf.changeset_id = c.id
               WHERE c.repo_id = $1
                 AND cf.file_path = $2
                 AND c.state IN ('submitted', 'verifying', 'approved')"#,
        )
        .bind(repo_id)
        .bind(path)
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
    /// Branch format: `{intent_slug}/{agent_name}`.
    fn slugify_intent(intent: &str) -> String {
        let slug: String = intent
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        if slug.len() > 50 {
            slug[..50].trim_end_matches('-').to_string()
        } else {
            slug
        }
    }

    #[test]
    fn source_branch_format_uses_intent_slug() {
        let intent = "Fix UI bugs";
        let agent_name = "agent-1";
        let source_branch = format!("{}/{}", slugify_intent(intent), agent_name);
        assert_eq!(source_branch, "fix-ui-bugs/agent-1");
    }

    #[test]
    fn source_branch_format_with_custom_name() {
        let intent = "Add comments endpoint";
        let agent_name = "feature-bot";
        let source_branch = format!("{}/{}", slugify_intent(intent), agent_name);
        assert_eq!(source_branch, "add-comments-endpoint/feature-bot");
    }

    #[test]
    fn target_branch_is_main() {
        // create() hardcodes target_branch to "main"
        let target_branch = "main";
        assert_eq!(target_branch, "main");
    }

    /// Test scaffolding fixture: a "draft" Changeset with sensible defaults
    /// (empty/None for nullable fields, "main" as target, now() for
    /// timestamps). Tests override only the fields they actually care about
    /// via struct-update syntax (`..test_changeset_fixture()`), which keeps
    /// each test focused and prevents every new struct field from forcing
    /// an edit across every test literal.
    fn test_changeset_fixture() -> Changeset {
        let now = Utc::now();
        Changeset {
            id: Uuid::new_v4(),
            repo_id: Uuid::new_v4(),
            number: 1,
            title: String::new(),
            intent_summary: None,
            source_branch: "agent/test".to_string(),
            target_branch: "main".to_string(),
            state: "draft".to_string(),
            reason: String::new(),
            session_id: None,
            agent_id: None,
            agent_name: None,
            author_id: None,
            base_version: None,
            merged_version: None,
            created_at: now,
            updated_at: now,
            merged_at: None,
            parent_changeset_id: None,
        }
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

        let cs = Changeset {
            repo_id,
            title: intent.to_string(),
            intent_summary: Some(intent.to_string()),
            source_branch: source_branch.clone(),
            session_id: Some(session_id),
            agent_id: Some(agent_id.to_string()),
            agent_name: Some(agent_id.to_string()),
            base_version: Some("abc123".to_string()),
            ..test_changeset_fixture()
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
        let id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();

        let cs = Changeset {
            id,
            repo_id,
            number: 42,
            title: "test".to_string(),
            source_branch: "agent/a".to_string(),
            ..test_changeset_fixture()
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
        let cs = Changeset {
            title: "clone test".to_string(),
            intent_summary: Some("intent".to_string()),
            source_branch: "agent/x".to_string(),
            agent_id: Some("x".to_string()),
            agent_name: Some("x".to_string()),
            ..test_changeset_fixture()
        };

        let cloned = cs.clone();
        assert_eq!(cs.id, cloned.id);
        assert_eq!(cs.source_branch, cloned.source_branch);
        assert_eq!(cs.target_branch, cloned.target_branch);
        assert_eq!(cs.state, cloned.state);
    }

    #[test]
    fn is_terminal_partitions_changeset_states() {
        assert!(!ChangesetState::Submitted.is_terminal());
        assert!(!ChangesetState::Verifying.is_terminal());
        assert!(!ChangesetState::Approved.is_terminal());
        assert!(ChangesetState::Merged.is_terminal());
        assert!(ChangesetState::Rejected.is_terminal());
        assert!(ChangesetState::Closed.is_terminal());
        assert!(ChangesetState::Draft.is_terminal()); // Draft: pre-submit, not worth pinning
    }
}
