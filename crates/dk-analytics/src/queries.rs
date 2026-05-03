//! Named, parameterized ClickHouse queries.
//!
//! Following pytorch/test-infra's `torchci/clickhouse_queries/<name>/query.sql`
//! convention, each analytical query lives in its own `.sql` file with
//! ClickHouse's native `{name:Type}` parameter syntax. Call sites bind
//! values with [`clickhouse::query::Query::param`] rather than interpolating
//! strings, which removes a whole class of injection bugs and lets operators
//! review / reuse the SQL directly.
//!
//! To add a new query:
//! 1. Drop a `queries/<name>.sql` file in this crate.
//! 2. Export a `pub const <NAME>: &str = include_str!("queries/<name>.sql");`
//!    from this module.
//! 3. Bind parameters in Rust via
//!    `client.query(queries::NAME).param("since", dt).fetch_one::<T>()`.

/// `SELECT count()` of changesets in state `merged` since `{since:DateTime64(3)}`.
pub const SUMMARY_MERGED_COUNT: &str = include_str!("queries/summary_merged_count.sql");

/// Arithmetic mean duration (ms) of verification steps since `{since:DateTime64(3)}`.
pub const SUMMARY_AVG_VERIFICATION_MS: &str =
    include_str!("queries/summary_avg_verification_ms.sql");

/// Review verdict distribution since `{since:DateTime64(3)}`. Each row is
/// `"verdict:count"`.
pub const SUMMARY_REVIEW_VERDICTS: &str = include_str!("queries/summary_review_verdicts.sql");

#[cfg(test)]
mod tests {
    use super::*;

    /// Every query must declare at least one `{param:Type}` binding so we
    /// don't accidentally regress to inline interpolation.
    #[test]
    fn all_named_queries_use_parameter_binding() {
        for (name, sql) in [
            ("SUMMARY_MERGED_COUNT", SUMMARY_MERGED_COUNT),
            ("SUMMARY_AVG_VERIFICATION_MS", SUMMARY_AVG_VERIFICATION_MS),
            ("SUMMARY_REVIEW_VERDICTS", SUMMARY_REVIEW_VERDICTS),
        ] {
            assert!(
                sql.contains("{since:DateTime64(3)}"),
                "{name} must bind `since` as DateTime64(3)"
            );
        }
    }
}
