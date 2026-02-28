mod client;
mod commands;
mod config;
mod util;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::remote::RemoteAction;

#[derive(Parser)]
#[command(name = "dk", about = "Dekode CLI — fast Git-compatible version control")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Clone a repository
    Clone {
        /// Repository URL or path
        url: String,
        /// Destination directory
        path: Option<PathBuf>,
    },

    /// Initialize a new repository
    Init {
        /// Directory to initialize (defaults to current directory)
        path: Option<PathBuf>,
    },

    /// Add file contents to the staging area
    Add {
        /// Files to add
        pathspec: Vec<PathBuf>,

        /// Add all changes (new, modified, deleted)
        #[arg(short = 'A', long)]
        all: bool,
    },

    /// Record changes to the repository
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Show commit history
    Log {
        /// One-line format
        #[arg(long)]
        oneline: bool,
        /// Limit number of commits
        #[arg(short)]
        n: Option<usize>,
    },

    /// Show changes between working tree, index, and HEAD
    Diff {
        /// Show staged changes (index vs HEAD)
        #[arg(long)]
        staged: bool,
        /// Limit to specific file
        path: Option<PathBuf>,
    },

    /// Push commits to a remote repository
    Push {
        /// Remote name (default: origin)
        remote: Option<String>,
        /// Branch name (default: current branch)
        branch: Option<String>,
    },

    /// Pull changes from a remote repository
    Pull {
        /// Remote name (default: origin)
        remote: Option<String>,
        /// Branch name (default: current branch)
        branch: Option<String>,
    },

    /// List, create, or delete branches
    Branch {
        /// Branch name to create
        name: Option<String>,
        /// Delete a branch
        #[arg(short, long)]
        delete: Option<String>,
        /// List all branches (including remote)
        #[arg(short, long)]
        all: bool,
    },

    /// Manage remote repositories
    Remote {
        #[command(subcommand)]
        action: Option<RemoteAction>,
        /// Verbose listing
        #[arg(short, long)]
        verbose: bool,
    },

    /// Reapply commits on top of another base
    Rebase {
        /// Branch to rebase onto
        branch: Option<String>,
        /// New base branch
        #[arg(long)]
        onto: Option<String>,
    },

    /// Merge a branch into the current branch
    Merge {
        /// Branch to merge
        branch: String,
    },

    /// Switch branches or create new branch
    Checkout {
        /// Branch or ref to switch to
        target: Option<String>,
        /// Create and switch to new branch
        #[arg(short)]
        b: Option<String>,
    },

    /// Create, list, or delete tags
    Tag {
        /// Tag name to create
        name: Option<String>,
        /// Tag message (creates annotated tag)
        #[arg(short, long)]
        message: Option<String>,
        /// Delete a tag
        #[arg(short, long)]
        delete: Option<String>,
        /// List tags
        #[arg(short, long)]
        list: bool,
    },

    /// Show working tree status
    Status,

    /// Log in to a Dekode server
    Login {
        /// Server URL (e.g. http://localhost:8080)
        url: String,
    },

    /// Log out from the Dekode server
    Logout,

    /// Show current login status
    Whoami,

    /// Manage repositories
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

    /// Search a repository semantically
    Search {
        /// Search query
        query: String,
        /// Repository name
        #[arg(long)]
        repo: String,
        /// Maximum results
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Get code context from a repository
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

    /// Agent Protocol commands (connect, submit, verify, merge, watch)
    Agent {
        #[command(subcommand)]
        action: commands::agent::AgentAction,
    },
}

#[derive(Subcommand)]
enum RepoAction {
    /// Create a new repository
    Create { name: String },
    /// List repositories
    List,
    /// Delete a repository
    Delete { name: String },
}

#[derive(Subcommand)]
enum FilesAction {
    /// Upload local files to a server repository
    Upload {
        /// Repository name
        #[arg(long)]
        repo: String,
        /// Paths to upload (files or directories)
        paths: Vec<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.no_color || std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
        // Propagate to child git processes so their output is also uncolored.
        std::env::set_var("NO_COLOR", "1");
    }

    match cli.command {
        Commands::Clone { url, path } => commands::clone::run(url, path),
        Commands::Add { pathspec, all } => commands::add::run(pathspec, all),
        Commands::Commit { message } => commands::commit::run(message),
        Commands::Diff { staged, path } => commands::diff::run(staged, path),
        Commands::Init { path } => commands::init::run(path),
        Commands::Log { oneline, n } => commands::log::run(oneline, n),
        Commands::Push { remote, branch } => commands::push::run(remote, branch),
        Commands::Pull { remote, branch } => commands::pull::run(remote, branch),
        Commands::Branch { name, delete, all } => commands::branch::run(name, delete, all),
        Commands::Checkout { target, b } => commands::checkout::run(target, b),
        Commands::Merge { branch } => commands::git_merge::run(branch),
        Commands::Rebase { branch, onto } => commands::rebase::run(branch, onto),
        Commands::Remote { action, verbose } => commands::remote::run(action, verbose),
        Commands::Tag { name, message, delete, list } => commands::tag::run(name, message, delete, list),
        Commands::Status => commands::status::run(),
        Commands::Login { url } => commands::login::run(url),
        Commands::Logout => commands::logout::run(),
        Commands::Whoami => commands::whoami::run(),
        Commands::Repo { action } => match action {
            RepoAction::Create { name } => commands::repo::create(name),
            RepoAction::List => commands::repo::list(),
            RepoAction::Delete { name } => commands::repo::delete(name),
        },
        Commands::Files { action } => match action {
            FilesAction::Upload { repo, paths } => commands::files::upload(repo, paths),
        },
        Commands::Index { repo } => commands::index::run(repo),
        Commands::Search { query, repo, limit } => commands::search::run(query, repo, limit),
        Commands::Context { query, repo, max_tokens } => commands::context::run(query, repo, max_tokens),
        Commands::Agent { action } => commands::agent::run(action),
    }
}
