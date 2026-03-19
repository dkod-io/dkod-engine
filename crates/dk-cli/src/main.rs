mod auth;
mod client;
mod commands;
mod config;
mod grpc;
mod output;
mod session;
mod util;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use commands::remote::RemoteAction;
use output::Output;

const DEFAULT_SERVER: &str = "https://agent.dkod.io:443";

#[derive(Parser)]
#[command(name = "dk", about = "dkod CLI — agent-native code platform", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    /// JSON output mode (for agents)
    #[arg(long, global = true)]
    json: bool,

    /// gRPC server address
    #[arg(long, global = true, env = "DKOD_GRPC_ADDR")]
    server: Option<String>,

    /// Suppress non-essential output
    #[arg(long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Open a session on a dkod codebase
    Init {
        /// Repository name (auto-detected from git remote if omitted)
        repo: Option<String>,
        /// Session intent description
        #[arg(long, default_value = "interactive session")]
        intent: String,
    },

    /// Semantic code search
    Search {
        /// Search query
        query: String,
        /// Context depth: signatures, full, or call_graph
        #[arg(long, default_value = "full")]
        depth: String,
        /// Max token budget
        #[arg(long, default_value = "4000")]
        max_tokens: u32,
    },

    /// Read a file from the workspace
    Cat {
        /// File path within the repository
        path: String,
    },

    /// Write a file to the workspace
    Add {
        /// File path within the repository
        path: String,
        /// Content to write (reads from stdin if omitted)
        #[arg(long)]
        content: Option<String>,
        /// Read content from a local file
        #[arg(long)]
        from: Option<PathBuf>,
    },

    /// List files in the workspace
    Ls {
        /// Path prefix to filter by
        prefix: Option<String>,
        /// Show only modified files
        #[arg(long)]
        modified: bool,
    },

    /// Submit changeset with intent description
    Commit {
        /// Intent / description of changes
        #[arg(short, long)]
        message: String,
    },

    /// Run verification pipeline
    Check,

    /// Merge verified changeset into a Git commit
    Push {
        /// Commit message (defaults to changeset intent)
        #[arg(short, long)]
        message: Option<String>,
        /// Bypass the recency-guard warning after user acknowledgement
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Show session state and pending changes
    Status,

    /// Show pending changes in the changeset
    Diff,

    /// Authenticate via browser (device flow)
    Login,

    /// Git operations (clone, branch, tag, etc.)
    Git {
        #[command(subcommand)]
        action: GitAction,
    },

    /// Low-level Agent Protocol commands
    Agent {
        #[command(subcommand)]
        action: commands::agent::AgentAction,
    },

    /// Manage repositories (create, list, delete)
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },

    /// Upload files to a repository
    Files {
        #[command(subcommand)]
        action: FilesAction,
    },

    /// Index a repository for semantic search
    Index {
        /// Repository name
        #[arg(long)]
        repo: String,
    },

    /// Get code context from a repository (legacy HTTP)
    Context {
        /// Query describing what context you need
        query: String,
        /// Repository name
        #[arg(long)]
        repo: String,
        /// Maximum token budget
        #[arg(long, default_value = "4000")]
        max_tokens: Option<usize>,
    },
}

#[derive(Subcommand)]
enum GitAction {
    /// Clone a repository
    Clone {
        url: String,
        path: Option<PathBuf>,
    },
    /// Initialize a new local repository
    #[command(name = "init")]
    GitInit {
        path: Option<PathBuf>,
    },
    /// Stage files
    #[command(name = "add")]
    GitAdd {
        pathspec: Vec<PathBuf>,
        #[arg(short = 'A', long)]
        all: bool,
    },
    /// Record changes
    #[command(name = "commit")]
    GitCommit {
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Show commit history
    Log {
        #[arg(long)]
        oneline: bool,
        #[arg(short)]
        n: Option<usize>,
    },
    /// Show changes
    #[command(name = "diff")]
    GitDiff {
        #[arg(long)]
        staged: bool,
        path: Option<PathBuf>,
    },
    /// Push to remote
    #[command(name = "push")]
    GitPush {
        remote: Option<String>,
        branch: Option<String>,
    },
    /// Pull from remote
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    /// Branch operations
    Branch {
        name: Option<String>,
        #[arg(short, long)]
        delete: Option<String>,
        #[arg(short, long)]
        all: bool,
    },
    /// Switch branches
    Checkout {
        target: Option<String>,
        #[arg(short)]
        b: Option<String>,
    },
    /// Merge branches
    Merge { branch: String },
    /// Rebase
    Rebase {
        branch: Option<String>,
        #[arg(long)]
        onto: Option<String>,
    },
    /// Manage remotes
    Remote {
        #[command(subcommand)]
        action: Option<RemoteAction>,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Tag operations
    Tag {
        name: Option<String>,
        #[arg(short, long)]
        message: Option<String>,
        #[arg(short, long)]
        delete: Option<String>,
        #[arg(short, long)]
        list: bool,
    },
    /// Show working tree status
    #[command(name = "status")]
    GitStatus,
    /// Login (email/password, legacy)
    #[command(name = "login")]
    LegacyLogin { url: String },
    /// Logout
    Logout,
    /// Show login status
    Whoami,
    /// Search (legacy HTTP)
    #[command(name = "search")]
    LegacySearch {
        query: String,
        #[arg(long)]
        repo: String,
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum RepoAction {
    Create { name: String },
    List,
    Delete { name: String },
}

#[derive(Subcommand)]
enum FilesAction {
    Upload {
        #[arg(long)]
        repo: String,
        paths: Vec<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.no_color || std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
        std::env::set_var("NO_COLOR", "1");
    }

    let out = Output::new(cli.json);
    let server = cli.server.unwrap_or_else(|| DEFAULT_SERVER.to_string());

    match cli.command {
        // ── Agent Protocol commands (async) ──────────────────
        Commands::Init { repo, intent } => {
            run_async(commands::session_init::run(out, &server, repo.as_deref(), &intent))
        }
        Commands::Search { query, depth, max_tokens } => {
            run_async(commands::session_search::run(out, &query, &depth, max_tokens))
        }
        Commands::Cat { path } => {
            run_async(commands::cat::run(out, &path))
        }
        Commands::Add { path, content, from } => {
            run_async(commands::session_add::run(out, &path, content, from))
        }
        Commands::Ls { prefix, modified } => {
            run_async(commands::ls::run(out, prefix.as_deref(), modified))
        }
        Commands::Commit { message } => {
            run_async(commands::session_commit::run(out, &message))
        }
        Commands::Check => {
            run_async(commands::check::run(out))
        }
        Commands::Push { message, force } => {
            run_async(commands::session_push::run(out, message.as_deref(), force))
        }
        Commands::Status => {
            run_async(commands::session_status::run(out))
        }
        Commands::Diff => {
            run_async(commands::session_diff::run(out))
        }
        Commands::Login => {
            run_async(commands::device_login::run(out, &server))
        }

        // ── Git subcommands ──────────────────────────────────
        Commands::Git { action } => match action {
            GitAction::Clone { url, path } => commands::clone::run(url, path),
            GitAction::GitInit { path } => commands::init::run(path),
            GitAction::GitAdd { pathspec, all } => commands::add::run(pathspec, all),
            GitAction::GitCommit { message } => commands::commit::run(message),
            GitAction::Log { oneline, n } => commands::log::run(oneline, n),
            GitAction::GitDiff { staged, path } => commands::diff::run(staged, path),
            GitAction::GitPush { remote, branch } => commands::push::run(remote, branch),
            GitAction::Pull { remote, branch } => commands::pull::run(remote, branch),
            GitAction::Branch { name, delete, all } => commands::branch::run(name, delete, all),
            GitAction::Checkout { target, b } => commands::checkout::run(target, b),
            GitAction::Merge { branch } => commands::git_merge::run(branch),
            GitAction::Rebase { branch, onto } => commands::rebase::run(branch, onto),
            GitAction::Remote { action, verbose } => commands::remote::run(action, verbose),
            GitAction::Tag { name, message, delete, list } => commands::tag::run(name, message, delete, list),
            GitAction::GitStatus => commands::status::run(),
            GitAction::LegacyLogin { url } => commands::login::run(url),
            GitAction::Logout => commands::logout::run(),
            GitAction::Whoami => commands::whoami::run(),
            GitAction::LegacySearch { query, repo, limit } => commands::search::run(query, repo, limit),
        },

        // ── Other ────────────────────────────────────────────
        Commands::Agent { action } => commands::agent::run(action),
        Commands::Repo { action } => match action {
            RepoAction::Create { name } => commands::repo::create(name),
            RepoAction::List => commands::repo::list(),
            RepoAction::Delete { name } => commands::repo::delete(name),
        },
        Commands::Files { action } => match action {
            FilesAction::Upload { repo, paths } => commands::files::upload(repo, paths),
        },
        Commands::Index { repo } => commands::index::run(repo),
        Commands::Context { query, repo, max_tokens } => commands::context::run(query, repo, max_tokens),
    }
}

fn run_async<F: std::future::Future<Output = Result<()>>>(fut: F) -> Result<()> {
    tokio::runtime::Runtime::new()
        .context("failed to create async runtime")
        .and_then(|rt| rt.block_on(fut))
}
