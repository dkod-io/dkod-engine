use sqlx::PgPool;
use uuid::Uuid;

use dk_core::RepoId;

#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub repo_id: RepoId,
    pub step_order: i32,
    pub step_type: String,
    pub config: serde_json::Value,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub id: Uuid,
    pub changeset_id: Uuid,
    pub step_order: i32,
    pub status: String,
    pub output: Option<String>,
}

pub struct PipelineStore {
    db: PgPool,
}

impl PipelineStore {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn get_pipeline(&self, repo_id: RepoId) -> dk_core::Result<Vec<PipelineStep>> {
        let rows: Vec<(Uuid, i32, String, serde_json::Value, bool)> = sqlx::query_as(
            "SELECT repo_id, step_order, step_type, config, required FROM verification_pipelines WHERE repo_id = $1 ORDER BY step_order",
        )
        .bind(repo_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| PipelineStep {
                repo_id: r.0,
                step_order: r.1,
                step_type: r.2,
                config: r.3,
                required: r.4,
            })
            .collect())
    }

    pub async fn set_pipeline(&self, repo_id: RepoId, steps: &[PipelineStep]) -> dk_core::Result<()> {
        sqlx::query("DELETE FROM verification_pipelines WHERE repo_id = $1")
            .bind(repo_id)
            .execute(&self.db)
            .await?;

        for step in steps {
            sqlx::query(
                r#"INSERT INTO verification_pipelines (repo_id, step_order, step_type, config, required)
                   VALUES ($1, $2, $3, $4, $5)"#,
            )
            .bind(repo_id)
            .bind(step.step_order)
            .bind(&step.step_type)
            .bind(&step.config)
            .bind(step.required)
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn record_result(
        &self,
        changeset_id: Uuid,
        step_order: i32,
        status: &str,
        output: Option<&str>,
    ) -> dk_core::Result<VerificationResult> {
        let row: (Uuid,) = sqlx::query_as(
            r#"INSERT INTO verification_results (changeset_id, step_order, status, output, started_at, completed_at)
               VALUES ($1, $2, $3, $4, now(), CASE WHEN $3 IN ('pass', 'fail', 'skip') THEN now() ELSE NULL END)
               RETURNING id"#,
        )
        .bind(changeset_id)
        .bind(step_order)
        .bind(status)
        .bind(output)
        .fetch_one(&self.db)
        .await?;

        Ok(VerificationResult {
            id: row.0,
            changeset_id,
            step_order,
            status: status.to_string(),
            output: output.map(String::from),
        })
    }

    pub async fn get_results(&self, changeset_id: Uuid) -> dk_core::Result<Vec<VerificationResult>> {
        let rows: Vec<(Uuid, Uuid, i32, String, Option<String>)> = sqlx::query_as(
            "SELECT id, changeset_id, step_order, status, output FROM verification_results WHERE changeset_id = $1 ORDER BY step_order",
        )
        .bind(changeset_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| VerificationResult {
                id: r.0,
                changeset_id: r.1,
                step_order: r.2,
                status: r.3,
                output: r.4,
            })
            .collect())
    }
}
