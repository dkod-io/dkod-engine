//! Integration tests for `dk-embedded-ch`.
//!
//! These tests spawn chDB in-process, which loads `libchdb.so`. They run
//! single-threaded because the underlying engine isn't safe to initialize
//! from multiple Rust test threads concurrently — pin to 1 thread per the
//! upstream `chdb-rust` README.

use std::path::PathBuf;

use dk_embedded_ch::format::OutputFormat;
use dk_embedded_ch::{execute, query_csv, query_json_each_row, query_to_parquet};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn execute_simple_arithmetic() {
    let out = execute("SELECT 1 + 1 AS sum", OutputFormat::JSONEachRow).unwrap();
    let text = out.as_utf8_lossy();
    assert!(
        text.contains(r#""sum":"2""#) || text.contains(r#""sum":2"#),
        "unexpected output: {text}"
    );
}

#[test]
fn query_csv_groups_by_level() {
    let out = query_csv(
        fixture("logs.csv"),
        "SELECT level, COUNT(*) AS n FROM $SOURCE GROUP BY level ORDER BY n DESC, level ASC",
    )
    .unwrap();

    let text = out.as_utf8_lossy();
    // fixture has: info ×5, error ×3, warn ×2
    let info_idx = text.find(r#""level":"info""#).expect("info row present");
    let error_idx = text.find(r#""level":"error""#).expect("error row present");
    let warn_idx = text.find(r#""level":"warn""#).expect("warn row present");
    assert!(
        info_idx < error_idx && error_idx < warn_idx,
        "rows should be sorted info (5), error (3), warn (2); got:\n{text}"
    );
}

#[test]
fn query_json_each_row_sums_committed_fixture() {
    let out =
        query_json_each_row(fixture("values.jsonl"), "SELECT SUM(a) AS s FROM $SOURCE").unwrap();
    let text = out.as_utf8_lossy();
    assert!(
        text.contains(r#""s":"6""#) || text.contains(r#""s":6"#),
        "expected sum=6, got: {text}"
    );
}

#[test]
fn query_to_parquet_emits_parquet_magic() {
    let out = query_to_parquet("SELECT number FROM numbers(8)").expect("query_to_parquet succeeds");
    let bytes = out.as_bytes();
    assert!(
        bytes.starts_with(b"PAR1"),
        "Parquet output should start with magic bytes 'PAR1'; got first 8 bytes: {:?}",
        &bytes[..bytes.len().min(8)]
    );
    assert!(
        bytes.ends_with(b"PAR1"),
        "Parquet output should end with magic bytes 'PAR1'"
    );
}
