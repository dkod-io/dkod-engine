use dk_core::{Dependency, RepoId, SymbolId};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// Intermediate row type for mapping between database rows and `Dependency`.
#[derive(sqlx::FromRow)]
struct DependencyRow {
    id: Uuid,
    repo_id: Uuid,
    package: String,
    version_req: String,
}

impl DependencyRow {
    fn into_dependency(self) -> Dependency {
        Dependency {
            id: self.id,
            repo_id: self.repo_id,
            package: self.package,
            version_req: self.version_req,
        }
    }
}

/// PostgreSQL-backed store for external dependency tracking.
#[derive(Clone)]
pub struct DependencyStore {
    pool: PgPool,
}

impl DependencyStore {
    /// Create a new `DependencyStore` backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert or update a dependency.
    ///
    /// Uses `ON CONFLICT (repo_id, package) DO UPDATE` so that
    /// re-parsing the same manifest file updates the version requirement.
    pub async fn upsert_dependency(&self, dep: &Dependency) -> dk_core::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO dependencies (id, repo_id, package, version_req)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (repo_id, package) DO UPDATE SET
                id = EXCLUDED.id,
                version_req = EXCLUDED.version_req
            "#,
        )
        .bind(dep.id)
        .bind(dep.repo_id)
        .bind(&dep.package)
        .bind(&dep.version_req)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find all dependencies for a given repository.
    pub async fn find_by_repo(&self, repo_id: RepoId) -> dk_core::Result<Vec<Dependency>> {
        let rows = sqlx::query_as::<_, DependencyRow>(
            r#"
            SELECT id, repo_id, package, version_req
            FROM dependencies
            WHERE repo_id = $1
            ORDER BY package
            "#,
        )
        .bind(repo_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(DependencyRow::into_dependency).collect())
    }

    /// Link a symbol to a dependency (records that the symbol uses/imports
    /// something from the given external package).
    pub async fn link_symbol_to_dep(
        &self,
        symbol_id: SymbolId,
        dep_id: Uuid,
    ) -> dk_core::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO symbol_dependencies (symbol_id, dependency_id)
            VALUES ($1, $2)
            ON CONFLICT (symbol_id, dependency_id) DO NOTHING
            "#,
        )
        .bind(symbol_id)
        .bind(dep_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find all symbol IDs that are linked to a specific dependency.
    pub async fn find_symbols_for_dep(
        &self,
        dep_id: Uuid,
    ) -> dk_core::Result<Vec<SymbolId>> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT symbol_id FROM symbol_dependencies WHERE dependency_id = $1",
        )
        .bind(dep_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Delete all dependencies for a repository. Returns the number of rows deleted.
    ///
    /// Note: this cascades to `symbol_dependencies` via foreign key constraints.
    pub async fn delete_by_repo(&self, repo_id: RepoId) -> dk_core::Result<u64> {
        let result = sqlx::query("DELETE FROM dependencies WHERE repo_id = $1")
            .bind(repo_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}
