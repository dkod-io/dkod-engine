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
    assert!(result
        .merged_content
        .contains("use std::collections::HashMap;"));
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

    let result = ast_merge(&registry(), "test.rs", RUST_BASE, version_a, RUST_BASE).unwrap();
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
    let result = ast_merge(&registry(), "test.rs", RUST_BASE, RUST_BASE, RUST_BASE).unwrap();
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

// ── Python merge tests ──

const PYTHON_BASE: &str = r#"import hashlib
import logging

# Module-level logger
logger = logging.getLogger(__name__)

# Rate limit config
MAX_ATTEMPTS = 5

def hash_password(password: str) -> str:
    return hashlib.sha256(password.encode()).hexdigest()

def verify_password(password: str, hashed: str) -> bool:
    return hash_password(password) == hashed

def generate_token(user_id: str) -> str:
    """Generate an authentication token."""
    return f"{user_id}:token"

def authenticate(email: str, password: str) -> bool:
    logger.info(f"Auth attempt for {email}")
    return True
"#;

#[test]
fn test_python_merge_different_functions_clean() {
    // Agent A modifies hash_password, Agent B modifies generate_token → clean merge
    let version_a = r#"import hashlib
import logging

# Module-level logger
logger = logging.getLogger(__name__)

# Rate limit config
MAX_ATTEMPTS = 5

def hash_password(password: str) -> str:
    """Hash using SHA-256. Added by Alpha."""
    return hashlib.sha256(password.encode()).hexdigest()

def verify_password(password: str, hashed: str) -> bool:
    return hash_password(password) == hashed

def generate_token(user_id: str) -> str:
    """Generate an authentication token."""
    return f"{user_id}:token"

def authenticate(email: str, password: str) -> bool:
    logger.info(f"Auth attempt for {email}")
    return True
"#;

    let version_b = r#"import hashlib
import logging

# Module-level logger
logger = logging.getLogger(__name__)

# Rate limit config
MAX_ATTEMPTS = 5

def hash_password(password: str) -> str:
    return hashlib.sha256(password.encode()).hexdigest()

def verify_password(password: str, hashed: str) -> bool:
    return hash_password(password) == hashed

def generate_token(user_id: str) -> str:
    """Generate a 24-hour token. Modified by Beta."""
    return f"{user_id}:token"

def authenticate(email: str, password: str) -> bool:
    logger.info(f"Auth attempt for {email}")
    return True
"#;

    let result = ast_merge(&registry(), "auth.py", PYTHON_BASE, version_a, version_b)
        .expect("ast_merge should succeed for Python");
    assert_eq!(
        result.status,
        MergeStatus::Clean,
        "Different functions should merge cleanly, got conflicts: {:?}",
        result.conflicts
    );
    // Both changes preserved
    assert!(
        result.merged_content.contains("Added by Alpha"),
        "Alpha's docstring should be in merged output"
    );
    assert!(
        result.merged_content.contains("Modified by Beta"),
        "Beta's docstring should be in merged output"
    );
}

#[test]
fn test_python_merge_preserves_comment_hash_prefix() {
    // Comments in merged output must retain their # prefix
    let version_a = PYTHON_BASE.replace(
        "def hash_password(password: str) -> str:\n    return hashlib.sha256(password.encode()).hexdigest()",
        "def hash_password(password: str) -> str:\n    \"\"\"Hash a password.\"\"\"\n    return hashlib.sha256(password.encode()).hexdigest()",
    );

    let result = ast_merge(&registry(), "auth.py", PYTHON_BASE, &version_a, PYTHON_BASE)
        .expect("ast_merge should succeed");

    // Comments must keep their # prefix
    assert!(
        result.merged_content.contains("# Module-level logger"),
        "Comment must retain # prefix, got:\n{}",
        result.merged_content
    );
    assert!(
        result.merged_content.contains("# Rate limit config"),
        "Comment must retain # prefix"
    );
}

#[test]
fn test_python_merge_preserves_symbol_order() {
    // Symbols must appear in original base order, not alphabetical
    let version_a = PYTHON_BASE.replace(
        "def hash_password(password: str) -> str:\n    return hashlib.sha256(password.encode()).hexdigest()",
        "def hash_password(password: str) -> str:\n    \"\"\"Hashes the password.\"\"\"\n    return hashlib.sha256(password.encode()).hexdigest()",
    );

    let result = ast_merge(&registry(), "auth.py", PYTHON_BASE, &version_a, PYTHON_BASE)
        .expect("ast_merge should succeed");

    // logger must appear BEFORE authenticate (which uses it)
    let logger_pos = result.merged_content.find("logger = logging.getLogger");
    let auth_pos = result.merged_content.find("def authenticate");
    assert!(
        logger_pos.is_some() && auth_pos.is_some(),
        "Both logger and authenticate must exist in output"
    );
    assert!(
        logger_pos.unwrap() < auth_pos.unwrap(),
        "logger must appear before authenticate (which uses it). logger at {}, authenticate at {}.\nOutput:\n{}",
        logger_pos.unwrap(),
        auth_pos.unwrap(),
        result.merged_content
    );
}

#[test]
fn test_python_merge_preserves_import_order() {
    // Imports must keep original order (stdlib before local)
    let version_a = PYTHON_BASE.replace(
        "import hashlib\nimport logging",
        "import hashlib\nimport logging\nimport os",
    );

    let result = ast_merge(&registry(), "auth.py", PYTHON_BASE, &version_a, PYTHON_BASE)
        .expect("ast_merge should succeed");

    let hashlib_pos = result.merged_content.find("import hashlib");
    let logging_pos = result.merged_content.find("import logging");
    let os_pos = result.merged_content.find("import os");
    assert!(
        hashlib_pos.is_some() && logging_pos.is_some() && os_pos.is_some(),
        "All three imports must exist; got:\n{}",
        result.merged_content
    );
    assert!(
        hashlib_pos.unwrap() < logging_pos.unwrap(),
        "import hashlib should come before import logging (base order preserved)"
    );
}

/// Reproduction test using the EXACT production auth.py structure:
/// - Inline comments (`# 60 seconds` at end of assignment)
/// - Multi-line comment blocks between variables
/// - Module-level logger before functions
/// - `from db import` mixed with stdlib imports
#[test]
fn test_python_merge_exact_production_file() {
    let base = concat!(
        "import datetime\n",
        "import hashlib\n",
        "import logging\n",
        "import secrets\n",
        "import time\n",
        "from db import get_user_by_email\n",
        "\n",
        "logger = logging.getLogger(__name__)\n",
        "\n",
        "# Rate limiting: track all login attempts per email\n",
        "# Each entry is a list of timestamps of attempts\n",
        "_login_attempts: dict[str, list[float]] = {}\n",
        "\n",
        "# Global rate limit\n",
        "_LOGIN_RATE_LIMIT_MAX = 5\n",
        "_LOGIN_RATE_LIMIT_WINDOW = 60  # 60 seconds\n",
        "\n",
        "LOCKOUT_THRESHOLD = 3\n",
        "\n",
        "\n",
        "def hash_password(password: str) -> str:\n",
        "    return hashlib.sha256(password.encode()).hexdigest()\n",
        "\n",
        "\n",
        "def verify_password(password: str, hashed: str) -> bool:\n",
        "    return hash_password(password) == hashed\n",
        "\n",
        "\n",
        "def generate_token(user_id: str) -> str:\n",
        "    \"\"\"Generate an authentication token with expiry timestamp.\"\"\"\n",
        "    expiry = datetime.datetime.utcnow() + datetime.timedelta(hours=24)\n",
        "    return f\"{user_id}:{expiry.isoformat()}\"\n",
        "\n",
        "\n",
        "def authenticate(email: str, password: str) -> dict | None:\n",
        "    \"\"\"Authenticate a user by email and password.\"\"\"\n",
        "    logger.info(f\"Authentication attempt for {email}\")\n",
        "    return None\n",
    );

    // Alpha: add docstring to hash_password only
    let version_a = base.replace(
        "def hash_password(password: str) -> str:\n    return hashlib.sha256(password.encode()).hexdigest()",
        "def hash_password(password: str) -> str:\n    \"\"\"Hash using SHA-256. [Alpha]\"\"\"\n    return hashlib.sha256(password.encode()).hexdigest()",
    );

    // Beta: modify generate_token docstring only
    let version_b = base.replace(
        "\"\"\"Generate an authentication token with expiry timestamp.\"\"\"",
        "\"\"\"Generate a 24-hour token. [Beta]\"\"\"",
    );

    let result = ast_merge(&registry(), "auth.py", base, &version_a, &version_b)
        .expect("ast_merge should succeed for production-like Python file");

    // Print full output for debugging
    eprintln!(
        "=== MERGED OUTPUT ===\n{}\n=== END ===",
        result.merged_content
    );

    assert_eq!(
        result.status,
        MergeStatus::Clean,
        "Different functions should merge cleanly, got conflicts: {:?}",
        result.conflicts
    );

    // Both changes preserved
    assert!(
        result.merged_content.contains("[Alpha]"),
        "Alpha's docstring must be in output"
    );
    assert!(
        result.merged_content.contains("[Beta]"),
        "Beta's docstring must be in output"
    );

    // Comments must keep # prefix
    assert!(
        result.merged_content.contains("# Rate limiting"),
        "Comment must retain # prefix. Output:\n{}",
        result.merged_content
    );
    assert!(
        result.merged_content.contains("# Global rate limit"),
        "Comment must retain # prefix"
    );

    // logger must be before authenticate
    let logger_pos = result.merged_content.find("logger = logging.getLogger");
    let auth_pos = result.merged_content.find("def authenticate");
    assert!(
        logger_pos.unwrap() < auth_pos.unwrap(),
        "logger must appear before authenticate. Output:\n{}",
        result.merged_content
    );

    // Imports: stdlib before local
    let hashlib_pos = result.merged_content.find("import hashlib").unwrap();
    let from_db_pos = result.merged_content.find("from db import").unwrap();
    assert!(
        hashlib_pos < from_db_pos,
        "stdlib imports must come before local imports. Output:\n{}",
        result.merged_content
    );

    // Inline comments must stay on the same line as their assignment
    assert!(
        result
            .merged_content
            .contains("_LOGIN_RATE_LIMIT_WINDOW = 60  # 60 seconds"),
        "Inline comment must stay on same line. Output:\n{}",
        result.merged_content
    );

    // No extra blank lines between consecutive variable assignments
    assert!(
        !result
            .merged_content
            .contains("_LOGIN_RATE_LIMIT_MAX = 5\n\n_LOGIN_RATE_LIMIT_WINDOW"),
        "Consecutive variables should not have blank line between them. Output:\n{}",
        result.merged_content
    );
}
