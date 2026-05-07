use std::path::PathBuf;

use dk_core::{RepoId, Span, Symbol, SymbolId, SymbolKind, Visibility};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// Intermediate row type for mapping between database rows and `Symbol`.
#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct SymbolRow {
    id: Uuid,
    repo_id: Uuid,
    name: String,
    qualified_name: String,
    kind: String,
    visibility: String,
    file_path: String,
    start_byte: i32,
    end_byte: i32,
    signature: Option<String>,
    doc_comment: Option<String>,
    parent_id: Option<Uuid>,
    last_modified_by: Option<String>,
    last_modified_intent: Option<String>,
}

impl SymbolRow {
    fn into_symbol(self) -> Symbol {
        Symbol {
            id: self.id,
            name: self.name,
            qualified_name: self.qualified_name,
            kind: self.kind.parse::<SymbolKind>().unwrap_or_else(|e| {
                tracing::warn!("{e}, defaulting to Variable");
                SymbolKind::Variable
            }),
            visibility: self.visibility.parse::<Visibility>().unwrap_or_else(|e| {
                tracing::warn!("{e}, defaulting to Private");
                Visibility::Private
            }),
            file_path: PathBuf::from(self.file_path),
            span: Span {
                start_byte: self.start_byte as u32,
                end_byte: self.end_byte as u32,
            },
            signature: self.signature,
            doc_comment: self.doc_comment,
            parent: self.parent_id,
            last_modified_by: self.last_modified_by,
            last_modified_intent: self.last_modified_intent,
        }
    }
}

/// PostgreSQL-backed CRUD store for the symbol table.
#[derive(Clone)]
pub struct SymbolStore {
    pool: PgPool,
}

impl SymbolStore {
    /// Create a new `SymbolStore` backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert or update a symbol.
    ///
    /// Uses `ON CONFLICT (repo_id, qualified_name) DO UPDATE` so that
    /// repeated ingestion of the same file is idempotent.
    pub async fn upsert_symbol(&self, repo_id: RepoId, sym: &Symbol) -> dk_core::Result<()> {
        self.upsert_symbols(repo_id, std::slice::from_ref(sym)).await
    }

    /// Insert or update multiple symbols.
    pub async fn upsert_symbols(&self, repo_id: RepoId, symbols: &[Symbol]) -> dk_core::Result<()> {
        if symbols.is_empty() {
            return Ok(());
        }

        let mut query_builder: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "INSERT INTO symbols (id, repo_id, name, qualified_name, kind, visibility, file_path, start_byte, end_byte, signature, doc_comment, parent_id, last_modified_by, last_modified_intent) "
        );

        query_builder.push_values(symbols, |mut b, sym| {
            b.push_bind(sym.id)
                .push_bind(repo_id)
                .push_bind(sym.name.clone())
                .push_bind(sym.qualified_name.clone())
                .push_bind(sym.kind.to_string())
                .push_bind(sym.visibility.to_string())
                .push_bind(sym.file_path.to_string_lossy().to_string())
                .push_bind(sym.span.start_byte as i32)
                .push_bind(sym.span.end_byte as i32)
                .push_bind(sym.signature.clone())
                .push_bind(sym.doc_comment.clone())
                .push_bind(sym.parent)
                .push_bind(sym.last_modified_by.clone())
                .push_bind(sym.last_modified_intent.clone());
        });

        query_builder.push(
            r#"
            ON CONFLICT (repo_id, qualified_name) DO UPDATE SET
                id = EXCLUDED.id,
                name = EXCLUDED.name,
                kind = EXCLUDED.kind,
                visibility = EXCLUDED.visibility,
                file_path = EXCLUDED.file_path,
                start_byte = EXCLUDED.start_byte,
                end_byte = EXCLUDED.end_byte,
                signature = EXCLUDED.signature,
                doc_comment = EXCLUDED.doc_comment,
                parent_id = EXCLUDED.parent_id,
                last_modified_by = EXCLUDED.last_modified_by,
                last_modified_intent = EXCLUDED.last_modified_intent
            "#,
        );

        let query = query_builder.build();
        query.execute(&self.pool).await?;

        Ok(())
    }

    /// Search symbols by name or qualified_name using ILIKE.
    pub async fn find_symbols(&self, repo_id: RepoId, query: &str) -> dk_core::Result<Vec<Symbol>> {
        let pattern = format!("%{query}%");
        let rows = sqlx::query_as::<_, SymbolRow>(
            r#"
            SELECT id, repo_id, name, qualified_name, kind, visibility,
                   file_path, start_byte, end_byte, signature, doc_comment,
                   parent_id, last_modified_by, last_modified_intent
            FROM symbols
            WHERE repo_id = $1 AND (name ILIKE $2 OR qualified_name ILIKE $2)
            ORDER BY qualified_name
            "#,
        )
        .bind(repo_id)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(SymbolRow::into_symbol).collect())
    }

    /// Find all symbols of a given kind in a repository.
    pub async fn find_by_kind(
        &self,
        repo_id: RepoId,
        kind: &SymbolKind,
    ) -> dk_core::Result<Vec<Symbol>> {
        let kind_str = kind.to_string();
        let rows = sqlx::query_as::<_, SymbolRow>(
            r#"
            SELECT id, repo_id, name, qualified_name, kind, visibility,
                   file_path, start_byte, end_byte, signature, doc_comment,
                   parent_id, last_modified_by, last_modified_intent
            FROM symbols
            WHERE repo_id = $1 AND kind = $2
            ORDER BY qualified_name
            "#,
        )
        .bind(repo_id)
        .bind(&kind_str)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(SymbolRow::into_symbol).collect())
    }

    /// Find all symbols in a given file.
    pub async fn find_by_file(
        &self,
        repo_id: RepoId,
        file_path: &str,
    ) -> dk_core::Result<Vec<Symbol>> {
        let rows = sqlx::query_as::<_, SymbolRow>(
            r#"
            SELECT id, repo_id, name, qualified_name, kind, visibility,
                   file_path, start_byte, end_byte, signature, doc_comment,
                   parent_id, last_modified_by, last_modified_intent
            FROM symbols
            WHERE repo_id = $1 AND file_path = $2
            ORDER BY start_byte
            "#,
        )
        .bind(repo_id)
        .bind(file_path)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(SymbolRow::into_symbol).collect())
    }

    /// Get a single symbol by its ID.
    pub async fn get_by_id(&self, id: SymbolId) -> dk_core::Result<Option<Symbol>> {
        let row = sqlx::query_as::<_, SymbolRow>(
            r#"
            SELECT id, repo_id, name, qualified_name, kind, visibility,
                   file_path, start_byte, end_byte, signature, doc_comment,
                   parent_id, last_modified_by, last_modified_intent
            FROM symbols
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(SymbolRow::into_symbol))
    }

    /// Fetch multiple symbols by their IDs in a single batch query.
    ///
    /// Returns symbols in arbitrary order. Symbols that do not exist are
    /// silently omitted.
    pub async fn get_by_ids(&self, ids: &[SymbolId]) -> dk_core::Result<Vec<Symbol>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query_as::<_, SymbolRow>(
            r#"
            SELECT id, repo_id, name, qualified_name, kind, visibility,
                   file_path, start_byte, end_byte, signature, doc_comment,
                   parent_id, last_modified_by, last_modified_intent
            FROM symbols
            WHERE id = ANY($1)
            "#,
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(SymbolRow::into_symbol).collect())
    }

    /// Delete all symbols belonging to a file. Returns the number of rows deleted.
    pub async fn delete_by_file(&self, repo_id: RepoId, file_path: &str) -> dk_core::Result<u64> {
        let result = sqlx::query("DELETE FROM symbols WHERE repo_id = $1 AND file_path = $2")
            .bind(repo_id)
            .bind(file_path)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Delete all symbols belonging to a repository. Returns the number of rows deleted.
    pub async fn delete_by_repo(&self, repo_id: RepoId) -> dk_core::Result<u64> {
        let result = sqlx::query("DELETE FROM symbols WHERE repo_id = $1")
            .bind(repo_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Count symbols in a repository.
    pub async fn count(&self, repo_id: RepoId) -> dk_core::Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM symbols WHERE repo_id = $1")
            .bind(repo_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }
}
