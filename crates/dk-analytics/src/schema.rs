//! DDL + naive migrator. Reads [`SCHEMA_SQL`] and runs each statement in
//! order against the configured ClickHouse client.

use anyhow::{Context, Result};

use crate::client::AnalyticsClient;

/// Full ClickHouse schema — kept inline so `dk analytics migrate` does not
/// depend on the binary's working directory.
pub const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Parse [`SCHEMA_SQL`] into a sequence of statements separated by `;`,
/// skipping SQL line comments. Exposed for tests and for operators who want
/// to preview what `migrate` will execute.
pub fn statements(schema: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in schema.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            continue;
        }
        buf.push_str(line);
        buf.push('\n');
        if line.trim_end().ends_with(';') {
            let stmt = buf.trim_end().trim_end_matches(';').trim().to_string();
            if !stmt.is_empty() {
                out.push(stmt);
            }
            buf.clear();
        }
    }
    let tail = buf.trim().trim_end_matches(';').trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// Run the DDL against ClickHouse. Idempotent because every statement uses
/// `CREATE TABLE IF NOT EXISTS`.
pub async fn migrate(client: &AnalyticsClient) -> Result<()> {
    for stmt in statements(SCHEMA_SQL) {
        tracing::debug!(target: "dk_analytics", "applying DDL: {stmt}");
        client
            .inner()
            .query(&stmt)
            .execute()
            .await
            .with_context(|| format!("applying DDL statement: {stmt}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_parses_schema_sql_into_four_tables() {
        let stmts = statements(SCHEMA_SQL);
        assert_eq!(stmts.len(), 4, "expected 4 DDL statements, got {stmts:?}");
        for table in [
            "session_events",
            "changeset_lifecycle",
            "verification_runs",
            "review_results",
        ] {
            assert!(
                stmts.iter().any(|s| s.contains(table)),
                "missing CREATE TABLE for {table}"
            );
        }
    }

    #[test]
    fn statements_strips_comments_and_trailing_semicolons() {
        let src = "-- leading comment\nCREATE TABLE x (a UInt8) ENGINE = Memory;\n-- trailing\nCREATE TABLE y (b UInt8) ENGINE = Memory";
        let stmts = statements(src);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("CREATE TABLE x"));
        assert!(stmts[1].starts_with("CREATE TABLE y"));
        assert!(!stmts[0].ends_with(';'));
    }
}
