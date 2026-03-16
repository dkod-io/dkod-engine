use std::collections::HashSet;

/// Validate that a name is safe to use in a shell command argument.
/// Only alphanumeric characters, hyphens, underscores, and dots are allowed.
/// Must not start with a hyphen (to prevent flag injection).
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Given a list of changed file paths and a base command, rewrite the command
/// to scope it to only the affected packages/crates.
/// Returns `None` if no scoping is possible (run the full command).
pub fn scope_command_to_changeset(command: &str, changed_files: &[String]) -> Option<String> {
    let trimmed = command.trim();

    // Rust: "cargo test" → "cargo test -p crate1 -p crate2"
    if trimmed.starts_with("cargo test")
        || trimmed.starts_with("cargo check")
        || trimmed.starts_with("cargo clippy")
    {
        let crates = extract_rust_crates(changed_files);
        if crates.is_empty() {
            return None;
        }
        let base = trimmed
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let rest: Vec<&str> = trimmed.split_whitespace().skip(2).collect();
        let pkg_flags: Vec<String> = crates.iter().map(|c| format!("-p {}", c)).collect();
        let mut parts = vec![base];
        parts.extend(pkg_flags.iter().map(|s| s.as_str().to_string()));
        parts.extend(rest.iter().map(|s| s.to_string()));
        return Some(parts.join(" "));
    }

    // TypeScript: "bun test" → "bun test dir1 dir2"
    if trimmed.starts_with("bun test") {
        let dirs = extract_ts_dirs(changed_files);
        if dirs.is_empty() {
            return None;
        }
        let mut parts = vec!["bun test".to_string()];
        parts.extend(dirs);
        return Some(parts.join(" "));
    }

    // Python: "pytest" → "pytest pkg1 pkg2"
    if trimmed.starts_with("pytest") || trimmed.starts_with("python -m pytest") {
        let pkgs = extract_python_packages(changed_files);
        if pkgs.is_empty() {
            return None;
        }
        let base = if trimmed.starts_with("python") {
            "python -m pytest"
        } else {
            "pytest"
        };
        let mut parts = vec![base.to_string()];
        parts.extend(pkgs);
        return Some(parts.join(" "));
    }

    None
}

fn extract_rust_crates(files: &[String]) -> Vec<String> {
    let mut crates: HashSet<String> = HashSet::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        if parts.len() >= 2 && parts[0] == "crates" && is_safe_name(parts[1]) {
            crates.insert(parts[1].to_string());
        }
    }
    let mut sorted: Vec<String> = crates.into_iter().collect();
    sorted.sort();
    sorted
}

fn extract_ts_dirs(files: &[String]) -> Vec<String> {
    let mut dirs: HashSet<String> = HashSet::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        if parts.len() >= 2 && parts[0] == "src" && is_safe_name(parts[1]) {
            dirs.insert(format!("src/{}", parts[1]));
        }
    }
    let mut sorted: Vec<String> = dirs.into_iter().collect();
    sorted.sort();
    sorted
}

fn extract_python_packages(files: &[String]) -> Vec<String> {
    let mut pkgs: HashSet<String> = HashSet::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        if !parts.is_empty() && parts[0] != "tests" && is_safe_name(parts[0]) {
            pkgs.insert(parts[0].to_string());
        }
    }
    let mut sorted: Vec<String> = pkgs.into_iter().collect();
    sorted.sort();
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_cargo_test() {
        let files = vec![
            "crates/dk-engine/src/repo.rs".into(),
            "crates/dk-core/src/lib.rs".into(),
        ];
        let scoped = scope_command_to_changeset("cargo test", &files).unwrap();
        assert!(scoped.contains("-p dk-core"));
        assert!(scoped.contains("-p dk-engine"));
    }

    #[test]
    fn test_scope_cargo_test_with_flags() {
        let files = vec!["crates/dk-cli/src/main.rs".into()];
        let scoped = scope_command_to_changeset("cargo test --release", &files).unwrap();
        assert!(scoped.contains("-p dk-cli"));
        assert!(scoped.contains("--release"));
    }

    #[test]
    fn test_scope_bun_test() {
        let files = vec![
            "src/components/Header.tsx".into(),
            "src/pages/Home.tsx".into(),
        ];
        let scoped = scope_command_to_changeset("bun test", &files).unwrap();
        assert!(scoped.contains("src/components"));
        assert!(scoped.contains("src/pages"));
    }

    #[test]
    fn test_scope_pytest() {
        let files = vec!["mypackage/module.py".into()];
        let scoped = scope_command_to_changeset("pytest", &files).unwrap();
        assert!(scoped.contains("mypackage"));
    }

    #[test]
    fn test_no_crates_returns_none() {
        let files = vec!["README.md".into()];
        assert!(scope_command_to_changeset("cargo test", &files).is_none());
    }

    #[test]
    fn test_unknown_command_returns_none() {
        let files = vec!["crates/dk-core/src/lib.rs".into()];
        assert!(scope_command_to_changeset("make test", &files).is_none());
    }

    #[test]
    fn test_malicious_path_rejected() {
        let files = vec!["crates/$(evil)/src/lib.rs".into()];
        assert!(scope_command_to_changeset("cargo test", &files).is_none());
    }

    #[test]
    fn test_flag_injection_rejected() {
        let files = vec!["--collect-only/foo.py".into()];
        assert!(scope_command_to_changeset("pytest", &files).is_none());
    }
}
