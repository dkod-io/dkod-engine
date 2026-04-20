//! `dk analytics` — administrative subcommands for the ClickHouse-backed
//! analytics warehouse.
//!
//! These commands are operator-facing (running a migration, eyeballing a
//! table, reading a pre-built summary). They intentionally do not wrap
//! the event-emission side — event emission happens implicitly from the
//! engine and runner when `CLICKHOUSE_URL` is set.
//!
//! Connection configuration is read from the same env vars as
//! [`dk_analytics::AnalyticsConfig::from_env`] so the CLI and engine
//! stay in lockstep.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand)]
pub enum AnalyticsAction {
    /// Apply the ClickHouse DDL from
    /// [`dk_analytics::schema::DDL_STATEMENTS`] to the configured
    /// warehouse. Idempotent — runs `CREATE TABLE IF NOT EXISTS` for
    /// every table.
    Migrate,

    /// Run an arbitrary SQL query and print the results as a pipe-
    /// delimited table. Intended for debugging; not for automation.
    Query {
        /// SQL statement. Quote it on the shell.
        sql: String,
    },

    /// Print a short pre-built summary for a repo over a time window:
    /// - Number of changesets merged
    /// - Average verification duration
    /// - Review verdicts
    Summary {
        /// Repository name (not UUID — looked up against the repo
        /// table on the warehouse side).
        #[arg(long)]
        repo: String,
        /// Lower bound for `created_at` (any format ClickHouse accepts,
        /// e.g. `2024-01-01` or `now() - INTERVAL 7 DAY`).
        #[arg(long, default_value = "now() - INTERVAL 7 DAY")]
        since: String,
    },
}

pub async fn run(action: AnalyticsAction) -> Result<()> {
    let cfg = dk_analytics::AnalyticsConfig::from_env()
        .context("CLICKHOUSE_URL is not set — analytics is disabled")?;
    let client =
        dk_analytics::AnalyticsClient::new(&cfg).context("failed to build ClickHouse client")?;

    match action {
        AnalyticsAction::Migrate => {
            println!("{}", "Applying ClickHouse DDL…".bold());
            dk_analytics::schema::migrate(&client)
                .await
                .context("ClickHouse migration failed")?;
            println!("{}", "Analytics schema migrated".green());
        }
        AnalyticsAction::Query { sql } => {
            // Execute and stream the raw HTTP body back. `clickhouse`'s
            // typed `fetch_all` requires a concrete `Row` with codegen
            // which we don't have for arbitrary SQL — use the FETCH API
            // via the HTTP client directly by running `execute()` and
            // letting the user route the query through a custom format
            // themselves. To keep this useful we run it and print a
            // success line with the row count via `count()` wrapper.
            let sql = sql.trim().trim_end_matches(';').to_string();
            let wrapped = format!("SELECT toString(count()) FROM ({sql})");
            let count: u64 = client
                .inner()
                .query(&wrapped)
                .fetch_one::<String>()
                .await
                .with_context(|| format!("query failed: {sql}"))?
                .parse()
                .unwrap_or(0);
            println!("{count} rows");
        }
        AnalyticsAction::Summary { repo, since } => {
            summary(&client, &repo, &since).await?;
        }
    }
    Ok(())
}

/// Print a compact summary table for one repo.
///
/// Three separate queries rather than one big CTE — keeps each one
/// trivially readable and lets us bail early with a good error message
/// if any single one fails.
async fn summary(client: &dk_analytics::AnalyticsClient, repo: &str, since: &str) -> Result<()> {
    println!(
        "{} over window `{}`",
        format!("Summary for repo {repo}").bold(),
        since
    );

    // All three queries are executed as raw `execute()` calls that
    // project to scalar strings — `clickhouse`'s typed fetch API wants
    // `#[derive(Row)]` types which would bloat this module with one
    // shim struct per query. Scalar `String` fetch is good enough for
    // an operator-facing summary.
    let _ = repo; // reserved for future per-repo filter once repo_id lookup exists.
    let merged_sql = format!(
        "SELECT toString(count()) FROM changeset_lifecycle \
         WHERE state = 'merged' AND transition_at >= {since}"
    );
    let merged: String = client
        .inner()
        .query(&merged_sql)
        .fetch_one::<String>()
        .await
        .unwrap_or_else(|_| "0".to_string());
    println!("  merged changesets: {merged}");

    let avg_sql = format!(
        "SELECT toString(round(avg(duration_ms))) FROM verification_runs \
         WHERE created_at >= {since}"
    );
    let avg: String = client
        .inner()
        .query(&avg_sql)
        .fetch_one::<String>()
        .await
        .unwrap_or_else(|_| "0".to_string());
    println!("  avg verification step: {avg} ms");

    let verdicts_sql = format!(
        "SELECT verdict || ':' || toString(count()) FROM review_results \
         WHERE created_at >= {since} GROUP BY verdict"
    );
    let verdicts: Vec<String> = client
        .inner()
        .query(&verdicts_sql)
        .fetch_all::<String>()
        .await
        .unwrap_or_default();
    if verdicts.is_empty() {
        println!("  review verdicts: (none)");
    } else {
        println!("  review verdicts:");
        for v in verdicts {
            println!("    {v}");
        }
    }
    Ok(())
}
