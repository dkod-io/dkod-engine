//! Heuristic test-shard determination.
//!
//! pytorch/test-infra has a server-side test-determination service that
//! returns shard metadata for a given list of changed files. For dkod
//! we currently run the determination *locally* with a lightweight
//! heuristic — this keeps the step usable even when the HUD is
//! unreachable, and avoids burning quota on every changeset.
//!
//! The heuristic is:
//! - Map each changed file's top-level directory (e.g. `torch/nn/...`)
//!   to a shard by the same name (`torch_nn`).
//! - If the file is outside any recognised shard root, it contributes
//!   to the synthetic `misc` shard.
//!
//! This is intentionally simple. Swap in server-side determination via
//! [`crate::steps::pytorch_ci::client::PytorchClient`] when you want
//! higher-fidelity mapping.

use std::collections::BTreeMap;

use serde::Serialize;

/// A named group of tests that should run together.
#[derive(Clone, Debug, Serialize)]
pub struct Shard {
    pub name: String,
    pub files: Vec<String>,
    pub tests: Vec<String>,
}

impl Shard {
    /// Returns true when every test listed for this shard appears in
    /// the supplied flaky-test list. In that case the caller can skip
    /// running the shard entirely and emit a finding.
    pub fn is_fully_flaky(&self, flaky: &[String]) -> bool {
        if self.tests.is_empty() {
            return false;
        }
        self.tests.iter().all(|t| flaky.iter().any(|f| f == t))
    }
}

/// Compute the shard mapping from a list of changed files and symbols.
///
/// `symbols` is currently advisory — the heuristic does not use them,
/// but keeping them in the signature lets the server-side variant add
/// symbol-level precision later without breaking callers.
pub fn determine_shards(files: &[String], _symbols: &[String]) -> Vec<Shard> {
    let mut by_shard: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in files {
        let shard = shard_for_path(f);
        by_shard.entry(shard).or_default().push(f.clone());
    }
    by_shard
        .into_iter()
        .map(|(name, files)| {
            let tests = files
                .iter()
                .filter(|p| p.contains("test") && p.ends_with(".py"))
                .cloned()
                .collect();
            Shard { name, files, tests }
        })
        .collect()
}

fn shard_for_path(path: &str) -> String {
    // Take the first two path components and slugify them. `torch/nn/x.py`
    // → `torch_nn`. Short paths (single component) fall into `misc`.
    let parts: Vec<&str> = path
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    if parts.len() < 2 {
        return "misc".to_string();
    }
    format!("{}_{}", sanitise(parts[0]), sanitise(parts[1]))
}

fn sanitise(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shards_group_by_top_level_dirs() {
        let files = vec![
            "torch/nn/linear.py".to_string(),
            "torch/nn/activation.py".to_string(),
            "torch/optim/adam.py".to_string(),
            "README.md".to_string(),
        ];
        let shards = determine_shards(&files, &[]);
        let names: Vec<&str> = shards.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"torch_nn"));
        assert!(names.contains(&"torch_optim"));
        assert!(names.contains(&"misc"));
    }

    #[test]
    fn fully_flaky_skips_shard() {
        let shard = Shard {
            name: "torch_nn".into(),
            files: vec!["torch/nn/test_foo.py".into()],
            tests: vec!["torch/nn/test_foo.py".into()],
        };
        let flaky = vec!["torch/nn/test_foo.py".to_string()];
        assert!(shard.is_fully_flaky(&flaky));
    }

    #[test]
    fn fully_flaky_is_false_when_tests_empty() {
        let shard = Shard {
            name: "misc".into(),
            files: vec!["README.md".into()],
            tests: vec![],
        };
        assert!(!shard.is_fully_flaky(&[]));
    }
}
