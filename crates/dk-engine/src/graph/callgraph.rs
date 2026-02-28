use dk_core::{CallEdge, CallKind, RepoId, SymbolId};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// Intermediate row type for mapping between database rows and `CallEdge`.
#[derive(sqlx::FromRow)]
struct CallEdgeRow {
    id: Uuid,
    repo_id: Uuid,
    caller_id: Uuid,
    callee_id: Uuid,
    kind: String,
}

impl CallEdgeRow {
    fn into_call_edge(self) -> CallEdge {
        CallEdge {
            id: self.id,
            repo_id: self.repo_id,
            caller: self.caller_id,
            callee: self.callee_id,
            kind: parse_call_kind(&self.kind),
        }
    }
}

fn parse_call_kind(s: &str) -> CallKind {
    match s {
        "direct_call" => CallKind::DirectCall,
        "method_call" => CallKind::MethodCall,
        "import" => CallKind::Import,
        "implements" => CallKind::Implements,
        "inherits" => CallKind::Inherits,
        "macro_invocation" => CallKind::MacroInvocation,
        other => {
            tracing::warn!("Unknown call kind: {other}, defaulting to DirectCall");
            CallKind::DirectCall
        }
    }
}

/// PostgreSQL-backed store for call graph edges.
#[derive(Clone)]
pub struct CallGraphStore {
    pool: PgPool,
}

impl CallGraphStore {
    /// Create a new `CallGraphStore` backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a call edge. Uses `ON CONFLICT DO NOTHING` so repeated
    /// insertion of the same edge is idempotent.
    pub async fn insert_edge(&self, edge: &CallEdge) -> dk_core::Result<()> {
        let kind_str = edge.kind.to_string();

        sqlx::query(
            r#"
            INSERT INTO call_edges (id, repo_id, caller_id, callee_id, kind)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (repo_id, caller_id, callee_id, kind) DO NOTHING
            "#,
        )
        .bind(edge.id)
        .bind(edge.repo_id)
        .bind(edge.caller)
        .bind(edge.callee)
        .bind(&kind_str)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find all edges where the given symbol is the callee (i.e. who calls this symbol).
    pub async fn find_callers(&self, symbol_id: SymbolId) -> dk_core::Result<Vec<CallEdge>> {
        let rows = sqlx::query_as::<_, CallEdgeRow>(
            r#"
            SELECT id, repo_id, caller_id, callee_id, kind
            FROM call_edges
            WHERE callee_id = $1
            ORDER BY caller_id
            "#,
        )
        .bind(symbol_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(CallEdgeRow::into_call_edge).collect())
    }

    /// Find all edges where the given symbol is the caller (i.e. what does this symbol call).
    pub async fn find_callees(&self, symbol_id: SymbolId) -> dk_core::Result<Vec<CallEdge>> {
        let rows = sqlx::query_as::<_, CallEdgeRow>(
            r#"
            SELECT id, repo_id, caller_id, callee_id, kind
            FROM call_edges
            WHERE caller_id = $1
            ORDER BY callee_id
            "#,
        )
        .bind(symbol_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(CallEdgeRow::into_call_edge).collect())
    }

    /// Delete all call edges where the caller symbol is in the given file.
    /// This joins on the `symbols` table to resolve the caller's file path.
    /// Returns the number of rows deleted.
    pub async fn delete_edges_for_file(
        &self,
        repo_id: RepoId,
        file_path: &str,
    ) -> dk_core::Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM call_edges ce
            USING symbols s
            WHERE ce.caller_id = s.id
              AND s.repo_id = $1
              AND s.file_path = $2
            "#,
        )
        .bind(repo_id)
        .bind(file_path)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}
