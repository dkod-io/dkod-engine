//! Embedded ClickHouse (chDB) wrapper for local, zero-copy analytics.
//!
//! This crate wraps [`chdb-rust`](https://docs.rs/chdb-rust) — the FFI bindings
//! to the in-process ClickHouse engine — and exposes a narrow, typed surface
//! that the rest of dkod uses for:
//!
//! - Ephemeral analytics against user-uploaded CSV / Parquet / JSON files
//!   (e.g. the planned `/dkod/analytics` drop-zone in clickhouse-monitoring).
//! - Sandboxed query execution in `dk-runner` verification pipelines where
//!   spinning up a networked ClickHouse is overkill.
//! - Agent tooling that wants fast aggregation over local artifacts without
//!   pushing them to ClickHouse Cloud first.
//!
//! # Installation prerequisites
//!
//! The underlying `libchdb.so` (≈540 MB) is fetched at build time by
//! `chdb-rust`'s `build.rs`, or loaded from `/usr/local/lib/libchdb.so` +
//! `/usr/local/include/chdb.h` if already present (preferred for CI).
//! Manual install: `curl -sL https://lib.chdb.io | bash`.
//!
//! # Zero-copy / columnar output
//!
//! chDB v2 exposes Arrow's C Data Interface internally, but the `chdb-rust`
//! 1.3 enum doesn't surface `OutputFormat::Arrow` yet — only Parquet.
//! [`query_to_parquet`] returns a ClickHouse-emitted Parquet blob that
//! `arrow-rs`, `polars`, or `datafusion` can ingest directly (Parquet carries
//! its own schema, so there's no separate descriptor to pass along). For
//! callers that want JSON rows (the `/dkod/analytics` drop-zone, MCP tools),
//! the CSV/Parquet/JSON helpers below emit `JSONEachRow`.

use std::path::Path;

use chdb_rust::arg::Arg;
use chdb_rust::execute as chdb_execute;
use chdb_rust::format::{InputFormat, OutputFormat};

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Wraps any error surfaced by `chdb-rust`.
    #[error("chDB error: {0}")]
    Chdb(#[from] chdb_rust::error::Error),

    /// A supplied path was not valid UTF-8 or could not be rendered for use
    /// inside a ClickHouse `file()` table function.
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Raw bytes produced by a query, in whichever [`OutputFormat`] was requested.
///
/// The buffer is owned: chDB hands us its result buffer and we copy it out so
/// the underlying FFI allocation is freed once the wrapping `QueryResult` on
/// the `chdb-rust` side drops. Callers parse this however they want —
/// `serde_json::from_slice` for `JSONEachRow`, `arrow::ipc::reader::*` for
/// `Arrow`, etc.
#[derive(Debug, Clone)]
pub struct QueryOutput {
    bytes: Vec<u8>,
    format: OutputFormat,
}

impl QueryOutput {
    /// The raw result bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume and return the raw bytes.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Lossy UTF-8 view of the result — useful for text formats like
    /// `JSONEachRow`, `TabSeparated`, or `Pretty`.
    #[inline]
    pub fn as_utf8_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }

    /// The format the result is encoded in.
    #[inline]
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Byte length of the result.
    #[inline]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the result is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Run an arbitrary SQL statement and return the result in the requested
/// output format.
///
/// This is the lowest-level entrypoint. Higher-level helpers ([`query_csv`],
/// [`query_parquet`], [`query_json_each_row`]) build their SQL on top of it.
///
/// ```no_run
/// use dk_embedded_ch::{execute, format::OutputFormat};
///
/// let out = execute("SELECT 1 + 1 AS sum", OutputFormat::JSONEachRow).unwrap();
/// assert!(out.as_utf8_lossy().contains(r#""sum":2"#));
/// ```
pub fn execute(sql: &str, format: OutputFormat) -> Result<QueryOutput> {
    tracing::debug!(%sql, ?format, "dk-embedded-ch: executing query");
    let result = chdb_execute(sql, Some(&[Arg::OutputFormat(format)]))?;
    Ok(QueryOutput {
        bytes: result.data_ref().to_vec(),
        format,
    })
}

/// Run a SQL query against a local CSV file using ClickHouse's `file()` table
/// function.
///
/// Assumes the file has a header row — the first line is parsed as column
/// names (`CSVWithNames`). For headerless CSV, drop down to [`execute`] and
/// build the `file('…', CSV, '<column_defs>')` call yourself.
///
/// The returned output is `JSONEachRow` — one JSON object per row, newline
/// separated — which is easy to stream and parse.
///
/// The caller's `sql` must reference the source via the literal string
/// `$SOURCE` (which will be textually replaced), e.g.:
///
/// ```no_run
/// use dk_embedded_ch::query_csv;
///
/// let out = query_csv(
///     "tests/fixtures/logs.csv",
///     "SELECT level, COUNT(*) AS n FROM $SOURCE GROUP BY level ORDER BY n DESC",
/// ).unwrap();
/// ```
pub fn query_csv<P: AsRef<Path>>(path: P, sql: &str) -> Result<QueryOutput> {
    let src = file_source(path.as_ref(), InputFormat::CSVWithNames)?;
    let rewritten = sql.replace("$SOURCE", &src);
    execute(&rewritten, OutputFormat::JSONEachRow)
}

/// Run a SQL query against a local Parquet file.
///
/// `$SOURCE` rewriting rules match [`query_csv`]. Result is `JSONEachRow`.
pub fn query_parquet<P: AsRef<Path>>(path: P, sql: &str) -> Result<QueryOutput> {
    let src = file_source(path.as_ref(), InputFormat::Parquet)?;
    let rewritten = sql.replace("$SOURCE", &src);
    execute(&rewritten, OutputFormat::JSONEachRow)
}

/// Run a SQL query against a local `JSONEachRow` file.
///
/// `$SOURCE` rewriting rules match [`query_csv`]. Result is `JSONEachRow`.
pub fn query_json_each_row<P: AsRef<Path>>(path: P, sql: &str) -> Result<QueryOutput> {
    let src = file_source(path.as_ref(), InputFormat::JSONEachRow)?;
    let rewritten = sql.replace("$SOURCE", &src);
    execute(&rewritten, OutputFormat::JSONEachRow)
}

/// Run a SQL query and return the result as ClickHouse-encoded Parquet.
///
/// Parquet is self-describing (schema + row groups in one file), which makes
/// it the practical zero-copy path for Rust consumers today: `arrow-rs`,
/// `polars`, and `datafusion` all parse it in-place without row-by-row
/// decoding. Prefer this over `JSONEachRow` whenever the caller wants
/// columnar data, plans to re-query with another engine, or is shipping the
/// payload across a process boundary.
///
/// ```no_run
/// use dk_embedded_ch::query_to_parquet;
///
/// let out = query_to_parquet("SELECT number, number * 2 AS doubled FROM numbers(1000)").unwrap();
/// // Feed `out.as_bytes()` into `parquet::arrow::ArrowReaderBuilder` or similar.
/// ```
pub fn query_to_parquet(sql: &str) -> Result<QueryOutput> {
    execute(sql, OutputFormat::Parquet)
}

/// Build the `file('<path>', <format>)` source expression used inside SQL.
fn file_source(path: &Path, format: InputFormat) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::InvalidPath(path.display().to_string()))?;

    // ClickHouse single-quoted string literals escape `'` as `\'` and `\` as `\\`.
    let escaped = path_str.replace('\\', "\\\\").replace('\'', "\\'");
    Ok(format!("file('{}', {})", escaped, format.as_str()))
}

/// Re-exports of the relevant `chdb-rust` types so consumers don't need a
/// direct dep just to specify formats.
pub mod format {
    pub use chdb_rust::format::{InputFormat, OutputFormat};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source_escapes_single_quote() {
        let src = file_source(Path::new("/tmp/it's a file.csv"), InputFormat::CSV).unwrap();
        assert_eq!(src, r"file('/tmp/it\'s a file.csv', CSV)");
    }

    #[test]
    fn file_source_escapes_backslash_before_quote() {
        let src = file_source(Path::new(r"C:\data\weird.csv"), InputFormat::CSV).unwrap();
        assert_eq!(src, r"file('C:\\data\\weird.csv', CSV)");
    }
}
