use dk_engine::conflict::{ast_merge, MergeStatus};
use dk_engine::parser::ParserRegistry;

fn registry() -> ParserRegistry {
    ParserRegistry::new()
}

// ── Rust test sources ──

const RUST_BASE: &str = r#"use std::io;

fn fn_a() -> i32 {
    42
}

fn fn_b(x: i32) -> i32 {
    x + 1
}
"#;

#[test]
fn test_merge_different_symbols_no_conflict() {
    // Agent A modifies fn_a, Agent B modifies fn_b → clean merge
    let version_a = r#"use std::io;

fn fn_a() -> i32 {
    100
}

fn fn_b(x: i32) -> i32 {
    x + 1
}
"#;

    let version_b = r#"use std::io;

fn fn_a() -> i32 {
    42
}

fn fn_b(x: i32) -> i32 {
    x * 2
}
"#;

    let result = ast_merge(&registry(), "test.rs", RUST_BASE, version_a, version_b).unwrap();
    assert_eq!(result.status, MergeStatus::Clean);
    assert!(result.conflicts.is_empty());
    // Merged should have A's fn_a and B's fn_b
    assert!(result.merged_content.contains("100"));
    assert!(result.merged_content.contains("x * 2"));
}

#[test]
fn test_merge_same_symbol_conflict() {
    // Both modify fn_a → conflict
    let version_a = r#"use std::io;

fn fn_a() -> i32 {
    100
}

fn fn_b(x: i32) -> i32 {
    x + 1
}
"#;

    let version_b = r#"use std::io;

fn fn_a() -> i32 {
    200
}

fn fn_b(x: i32) -> i32 {
    x + 1
}
"#;

    let result = ast_merge(&registry(), "test.rs", RUST_BASE, version_a, version_b).unwrap();
    assert_eq!(result.status, MergeStatus::Conflict);
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].qualified_name, "fn_a");
    assert!(result.conflicts[0].version_a.contains("100"));
    assert!(result.conflicts[0].version_b.contains("200"));
    assert!(result.conflicts[0].base.contains("42"));
}

#[test]
fn test_merge_shifted_lines_no_conflict() {
    // Agent A grows fn_a by many lines (shifting fn_b down),
    // Agent B modifies fn_b → should be a clean merge
    let version_a = r#"use std::io;

fn fn_a() -> i32 {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    let f = 6;
    let g = 7;
    let h = 8;
    let i = 9;
    let j = 10;
    let k = 11;
    let l = 12;
    let m = 13;
    let n = 14;
    let o = 15;
    let p = 16;
    let q = 17;
    let r = 18;
    let s = 19;
    let t = 20;
    a + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p + q + r + s + t
}

fn fn_b(x: i32) -> i32 {
    x + 1
}
"#;

    let version_b = r#"use std::io;

fn fn_a() -> i32 {
    42
}

fn fn_b(x: i32) -> i32 {
    x * 10 + 5
}
"#;

    let result = ast_merge(&registry(), "test.rs", RUST_BASE, version_a, version_b).unwrap();
    assert_eq!(result.status, MergeStatus::Clean);
    assert!(result.conflicts.is_empty());
    // A's expanded fn_a should be present
    assert!(result.merged_content.contains("let t = 20;"));
    // B's modified fn_b should be present
    assert!(result.merged_content.contains("x * 10 + 5"));
}

#[test]
fn test_merge_imports_additive() {
    // Both add different imports → merged union
    let base = r#"use std::io;

fn helper() -> bool {
    true
}
"#;

    let version_a = r#"use std::io;
use std::collections::HashMap;

fn helper() -> bool {
    true
}
"#;

    let version_b = r#"use std::io;
use std::fmt;

fn helper() -> bool {
    true
}
"#;

    let result = ast_merge(&registry(), "test.rs", base, version_a, version_b).unwrap();
    assert_eq!(result.status, MergeStatus::Clean);
    assert!(result.merged_content.contains("use std::io;"));
    assert!(result.merged_content.contains("use std::collections::HashMap;"));
    assert!(result.merged_content.contains("use std::fmt;"));
}

#[test]
fn test_merge_new_symbol_by_one_agent() {
    // Agent A adds fn_c (not in base) → included in merge
    let version_a = r#"use std::io;

fn fn_a() -> i32 {
    42
}

fn fn_b(x: i32) -> i32 {
    x + 1
}

fn fn_c() -> String {
    "hello".to_string()
}
"#;

    let result =
        ast_merge(&registry(), "test.rs", RUST_BASE, version_a, RUST_BASE).unwrap();
    assert_eq!(result.status, MergeStatus::Clean);
    assert!(result.conflicts.is_empty());
    assert!(result.merged_content.contains("fn fn_c()"));
    assert!(result.merged_content.contains("\"hello\""));
}

#[test]
fn test_merge_both_add_same_named_symbol() {
    // Both add fn_c → conflict
    let version_a = r#"use std::io;

fn fn_a() -> i32 {
    42
}

fn fn_b(x: i32) -> i32 {
    x + 1
}

fn fn_c() -> i32 {
    1
}
"#;

    let version_b = r#"use std::io;

fn fn_a() -> i32 {
    42
}

fn fn_b(x: i32) -> i32 {
    x + 1
}

fn fn_c() -> i32 {
    2
}
"#;

    let result = ast_merge(&registry(), "test.rs", RUST_BASE, version_a, version_b).unwrap();
    assert_eq!(result.status, MergeStatus::Conflict);
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].qualified_name, "fn_c");
    // Base should be empty since fn_c didn't exist in base
    assert!(result.conflicts[0].base.is_empty());
}

#[test]
fn test_merge_unmodified_file() {
    // No changes — base returned
    let result =
        ast_merge(&registry(), "test.rs", RUST_BASE, RUST_BASE, RUST_BASE).unwrap();
    assert_eq!(result.status, MergeStatus::Clean);
    assert!(result.conflicts.is_empty());
    // Content should contain both functions
    assert!(result.merged_content.contains("fn fn_a()"));
    assert!(result.merged_content.contains("fn fn_b("));
}

#[test]
fn test_merge_struct_and_function() {
    // Agent A modifies struct, Agent B modifies function → clean
    let base = r#"struct Config {
    name: String,
}

fn process(c: &Config) -> bool {
    !c.name.is_empty()
}
"#;

    let version_a = r#"struct Config {
    name: String,
    debug: bool,
}

fn process(c: &Config) -> bool {
    !c.name.is_empty()
}
"#;

    let version_b = r#"struct Config {
    name: String,
}

fn process(c: &Config) -> bool {
    c.name.len() > 3
}
"#;

    let result = ast_merge(&registry(), "test.rs", base, version_a, version_b).unwrap();
    assert_eq!(result.status, MergeStatus::Clean);
    assert!(result.conflicts.is_empty());
    // A's struct change
    assert!(result.merged_content.contains("debug: bool"));
    // B's function change
    assert!(result.merged_content.contains("c.name.len() > 3"));
}

#[test]
fn test_merge_unsupported_file_extension() {
    let result = ast_merge(&registry(), "test.xyz", "base", "a", "b");
    assert!(result.is_err());
}

#[test]
fn test_merge_ts_expression_statements_different_routes() {
    // Two agents modify different route handlers (expression_statement nodes)
    // in the same TypeScript file → should merge cleanly, not file-level conflict.
    let base = r#"import { Router } from "express";

const router = Router();

router.get("/health", (_req, res) => {
  res.json({ status: "ok" });
});

router.get("/notes/:id", (req, res) => {
  const note = notes.find((n) => n.id === parseInt(req.params.id));
  res.json({ data: note });
});

export default router;
"#;

    let version_a = r#"import { Router } from "express";

const router = Router();

// Health endpoint — returns service status [Alice]
router.get("/health", (_req, res) => {
  res.json({ status: "ok" });
});

router.get("/notes/:id", (req, res) => {
  const note = notes.find((n) => n.id === parseInt(req.params.id));
  res.json({ data: note });
});

export default router;
"#;

    let version_b = r#"import { Router } from "express";

const router = Router();

router.get("/health", (_req, res) => {
  res.json({ status: "ok" });
});

// Fetch a single note by ID [Bob]
router.get("/notes/:id", (req, res) => {
  const note = notes.find((n) => n.id === parseInt(req.params.id));
  res.json({ data: note });
});

export default router;
"#;

    let result = ast_merge(&registry(), "routes.ts", base, version_a, version_b)
        .expect("ast_merge should succeed for TypeScript");
    assert_eq!(
        result.status,
        MergeStatus::Clean,
        "Different route handlers should merge cleanly, got conflicts: {:?}",
        result.conflicts
    );
    assert!(
        result.merged_content.contains("[Alice]"),
        "Alice's change should be in merged output"
    );
    assert!(
        result.merged_content.contains("[Bob]"),
        "Bob's change should be in merged output"
    );
    assert!(
        result.merged_content.contains("export default router"),
        "default export should be preserved in merged output"
    );
}
