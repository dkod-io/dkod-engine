use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use dk_protocol::agent_service_client::AgentServiceClient;
use dk_protocol::{
    Change as ProtoChange, ChangeType, ContextDepth, ContextRequest, FileListRequest,
    FileReadRequest, FileWriteRequest, MergeRequest, PreSubmitCheckRequest, SubmitRequest,
    VerifyRequest, WatchRequest,
};
use tokio_stream::StreamExt;
use tonic::transport::Channel;

use crate::config::Config;

#[derive(Subcommand)]
pub enum AgentAction {
    /// Connect to a Dekode server and start an agent session
    Connect {
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
        /// Repository name
        #[arg(long)]
        repo: String,
        /// Intent description
        #[arg(long)]
        intent: String,
        /// Auth token (overrides config)
        #[arg(long)]
        token: Option<String>,
    },

    /// Query the semantic code graph
    Context {
        /// Session ID from dk agent connect
        #[arg(long)]
        session: String,
        /// Search query
        query: String,
        /// Depth: signatures, full, or call_graph
        #[arg(long, default_value = "full")]
        depth: String,
        /// Max token budget
        #[arg(long, default_value = "4000")]
        max_tokens: u32,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Submit file changes to the current changeset
    Submit {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Changeset ID
        #[arg(long)]
        changeset: String,
        /// Files to submit
        files: Vec<PathBuf>,
        /// Intent description
        #[arg(long, default_value = "code changes")]
        intent: String,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Run verification pipeline on changeset
    Verify {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Changeset ID
        #[arg(long)]
        changeset: String,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Merge changeset into a Git commit
    Merge {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Changeset ID
        #[arg(long)]
        changeset: String,
        /// Commit message
        #[arg(short, long)]
        message: String,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Watch for repository events
    Watch {
        /// Session ID
        #[arg(long)]
        session: String,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Read a file from the session workspace
    FileRead {
        /// Session ID
        #[arg(long)]
        session: String,
        /// File path within the repository
        path: String,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Write a file into the session workspace
    FileWrite {
        /// Session ID
        #[arg(long)]
        session: String,
        /// File path within the repository
        #[arg(long)]
        path: String,
        /// Local file to read content from
        source: PathBuf,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// List files in the session workspace
    FileList {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Optional path prefix to filter by
        #[arg(long)]
        prefix: Option<String>,
        /// Show only files modified in this session
        #[arg(long)]
        only_modified: bool,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },

    /// Run pre-submit conflict check
    PreSubmit {
        /// Session ID
        #[arg(long)]
        session: String,
        /// gRPC server address
        #[arg(long, default_value = "http://[::1]:50051")]
        server: String,
    },
}

pub fn run(action: AgentAction) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to create async runtime")?;
    rt.block_on(run_async(action))
}

async fn run_async(action: AgentAction) -> Result<()> {
    match action {
        AgentAction::Connect {
            server,
            repo,
            intent,
            token,
        } => connect(server, repo, intent, token).await,

        AgentAction::Context {
            session,
            query,
            depth,
            max_tokens,
            server,
        } => context_cmd(server, session, query, depth, max_tokens).await,

        AgentAction::Submit {
            session,
            changeset,
            files,
            intent,
            server,
        } => submit_cmd(server, session, changeset, files, intent).await,

        AgentAction::Verify {
            session,
            changeset,
            server,
        } => verify_cmd(server, session, changeset).await,

        AgentAction::Merge {
            session,
            changeset,
            message,
            server,
        } => merge_cmd(server, session, changeset, message).await,

        AgentAction::Watch { session, server } => watch_cmd(server, session).await,

        AgentAction::FileRead {
            session,
            path,
            server,
        } => file_read_cmd(server, session, path).await,

        AgentAction::FileWrite {
            session,
            path,
            source,
            server,
        } => file_write_cmd(server, session, path, source).await,

        AgentAction::FileList {
            session,
            prefix,
            only_modified,
            server,
        } => file_list_cmd(server, session, prefix, only_modified).await,

        AgentAction::PreSubmit { session, server } => pre_submit_cmd(server, session).await,
    }
}

// ── CONNECT ──────────────────────────────────────────────────────────────────

async fn connect(
    server: String,
    repo: String,
    intent: String,
    token: Option<String>,
) -> Result<()> {
    let auth_token = resolve_token(token)?;

    let mut client = dk_agent_sdk::AgentClient::connect(&server, &auth_token)
        .await
        .context("failed to connect \u{2014} is dk-server running?")?;

    let session = client
        .init(&repo, &intent)
        .await
        .context("CONNECT handshake failed")?;

    println!(
        "{} Session: {}  Changeset: {}",
        "Connected.".green().bold(),
        session.session_id,
        session.changeset_id,
    );
    println!("  Server:  {}", server);
    println!("  Repo:    {}", repo);
    println!("  Version: {}", session.codebase_version);

    Ok(())
}

// ── CONTEXT ──────────────────────────────────────────────────────────────────

async fn context_cmd(
    server: String,
    session: String,
    query: String,
    depth: String,
    max_tokens: u32,
) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let proto_depth = match depth.as_str() {
        "signatures" => ContextDepth::Signatures as i32,
        "call_graph" => ContextDepth::CallGraph as i32,
        _ => ContextDepth::Full as i32,
    };

    let resp = client
        .context(ContextRequest {
            session_id: session,
            query,
            depth: proto_depth,
            include_tests: false,
            include_dependencies: false,
            max_tokens,
        })
        .await?
        .into_inner();

    if resp.symbols.is_empty() {
        println!("No symbols found.");
        return Ok(());
    }

    println!(
        "{:>3} | {:<10} | {:<40} | File:Offset",
        "#", "Kind", "Symbol",
    );
    println!("{}", "-".repeat(80));
    for (i, sym) in resp.symbols.iter().enumerate() {
        let symbol = sym.symbol.as_ref();
        let kind = symbol.map(|s| s.kind.as_str()).unwrap_or("?");
        let qname = symbol
            .map(|s| s.qualified_name.as_str())
            .unwrap_or("unknown");
        let file = symbol.map(|s| s.file_path.as_str()).unwrap_or("?");
        let offset = symbol.map(|s| s.start_byte).unwrap_or(0);
        println!(
            "{:>3} | {:<10} | {:<40} | {}:{}",
            i + 1,
            kind,
            qname,
            file,
            offset
        );
    }
    println!("\nEstimated tokens: {}", resp.estimated_tokens);

    Ok(())
}

// ── SUBMIT ───────────────────────────────────────────────────────────────────

async fn submit_cmd(
    server: String,
    session: String,
    changeset: String,
    files: Vec<PathBuf>,
    intent: String,
) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let mut changes = Vec::new();
    for path in &files {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let file_path = path
            .to_str()
            .context("non-UTF-8 file path")?
            .to_string();
        changes.push(ProtoChange {
            r#type: ChangeType::AddFunction as i32,
            symbol_name: String::new(),
            file_path,
            old_symbol_id: None,
            new_source: content,
            rationale: String::new(),
        });
    }

    let resp = client
        .submit(SubmitRequest {
            session_id: session,
            intent,
            changes,
            changeset_id: changeset,
        })
        .await?
        .into_inner();

    let status_str = format!("{:?}", resp.status());
    if resp.errors.is_empty() {
        println!(
            "{} changeset={} status={}",
            "Submitted.".green().bold(),
            resp.changeset_id,
            status_str,
        );
    } else {
        println!(
            "{} changeset={} status={}",
            "Submit returned errors.".red().bold(),
            resp.changeset_id,
            status_str,
        );
        for err in &resp.errors {
            let loc = err.file_path.as_deref().unwrap_or("?");
            println!("  {} {}: {}", "error:".red(), loc, err.message);
        }
    }

    Ok(())
}

// ── VERIFY ───────────────────────────────────────────────────────────────────

async fn verify_cmd(server: String, session: String, changeset: String) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let mut stream = client
        .verify(VerifyRequest {
            session_id: session,
            changeset_id: changeset,
        })
        .await?
        .into_inner();

    println!(
        "{:>3} | {:<24} | {:<10} | Output",
        "#", "Step", "Status"
    );
    println!("{}", "-".repeat(80));

    while let Some(step) = stream.next().await {
        let step = step.context("verify stream error")?;
        let status_display = match step.status.as_str() {
            "passed" | "PASSED" => "PASSED".green().bold().to_string(),
            "failed" | "FAILED" => "FAILED".red().bold().to_string(),
            "skipped" | "SKIPPED" => "SKIPPED".yellow().to_string(),
            other => other.to_string(),
        };
        let required = if step.required { "" } else { " (optional)" };
        println!(
            "{:>3} | {:<24} | {:<10} | {}{}",
            step.step_order, step.step_name, status_display, step.output, required,
        );
    }

    Ok(())
}

// ── MERGE ────────────────────────────────────────────────────────────────────

async fn merge_cmd(
    server: String,
    session: String,
    changeset: String,
    message: String,
) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let resp = client
        .merge(MergeRequest {
            session_id: session,
            changeset_id: changeset,
            commit_message: message,
        })
        .await?
        .into_inner();

    if resp.conflicts.is_empty() {
        println!(
            "{} commit={}  version={}",
            "Merged.".green().bold(),
            resp.commit_hash,
            resp.merged_version,
        );
    } else {
        println!(
            "{} {} conflict(s):",
            "Merge blocked.".red().bold(),
            resp.conflicts.len()
        );
        for c in &resp.conflicts {
            println!(
                "  {} {} ({}) \u{2014} {}",
                "conflict:".red(),
                c.file_path,
                c.conflict_type,
                c.description
            );
        }
    }

    Ok(())
}

// ── WATCH ────────────────────────────────────────────────────────────────────

async fn watch_cmd(server: String, session: String) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let mut stream = client
        .watch(WatchRequest {
            session_id: session,
            repo_id: String::new(),
            filter: "all".to_string(),
        })
        .await?
        .into_inner();

    println!("{}", "Watching for events (Ctrl+C to stop)...".cyan());

    while let Some(event) = stream.next().await {
        let event = event.context("watch stream error")?;
        let symbols = if event.affected_symbols.is_empty() {
            String::new()
        } else {
            format!(" symbols=[{}]", event.affected_symbols.join(", "))
        };
        println!(
            "{} [{}] agent={} changeset={}{}  {}",
            "\u{25cf}".cyan(),
            event.event_type,
            event.agent_id,
            event.changeset_id,
            symbols,
            event.details,
        );
    }

    Ok(())
}

// ── FILE-READ ────────────────────────────────────────────────────────────────

async fn file_read_cmd(server: String, session: String, path: String) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let resp = client
        .file_read(FileReadRequest {
            session_id: session,
            path: path.clone(),
        })
        .await?
        .into_inner();

    let modified_tag = if resp.modified_in_session {
        " (modified in session)".yellow().to_string()
    } else {
        String::new()
    };

    println!(
        "{} {}  hash={}{}",
        "File:".green().bold(),
        path,
        resp.hash,
        modified_tag,
    );

    // Try to display as UTF-8 text; fall back to hex summary for binary
    match String::from_utf8(resp.content.clone()) {
        Ok(text) => print!("{}", text),
        Err(_) => {
            let len = resp.content.len();
            let preview: String = resp.content.iter().take(64).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
            println!("[binary: {} bytes]  {}{}", len, preview, if len > 64 { " ..." } else { "" });
        }
    }

    Ok(())
}

// ── FILE-WRITE ───────────────────────────────────────────────────────────────

async fn file_write_cmd(
    server: String,
    session: String,
    path: String,
    source: PathBuf,
) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let content = std::fs::read(&source)
        .with_context(|| format!("failed to read local file {}", source.display()))?;

    let resp = client
        .file_write(FileWriteRequest {
            session_id: session,
            path: path.clone(),
            content,
        })
        .await?
        .into_inner();

    println!(
        "{} {}  new_hash={}",
        "Written.".green().bold(),
        path,
        resp.new_hash,
    );

    if !resp.detected_changes.is_empty() {
        println!("Detected symbol changes:");
        for change in &resp.detected_changes {
            println!("  {} {} ({})", "\u{2022}".cyan(), change.symbol_name, change.change_type);
        }
    }

    Ok(())
}

// ── FILE-LIST ────────────────────────────────────────────────────────────────

async fn file_list_cmd(
    server: String,
    session: String,
    prefix: Option<String>,
    only_modified: bool,
) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let resp = client
        .file_list(FileListRequest {
            session_id: session,
            prefix,
            only_modified,
        })
        .await?
        .into_inner();

    if resp.files.is_empty() {
        println!("No files found.");
        return Ok(());
    }

    println!("{:>4}  {:<6}  Path", "#", "Status");
    println!("{}", "-".repeat(60));
    for (i, entry) in resp.files.iter().enumerate() {
        let status = if entry.modified_in_session {
            "M".yellow().bold().to_string()
        } else {
            " ".to_string()
        };
        println!("{:>4}  {:<6}  {}", i + 1, status, entry.path);
    }
    println!("\n{} file(s)", resp.files.len());

    Ok(())
}

// ── PRE-SUBMIT ───────────────────────────────────────────────────────────────

async fn pre_submit_cmd(server: String, session: String) -> Result<()> {
    let mut client = grpc_client(&server).await?;

    let resp = client
        .pre_submit_check(PreSubmitCheckRequest {
            session_id: session,
        })
        .await?
        .into_inner();

    if resp.has_conflicts {
        println!(
            "{} {} conflict(s) detected",
            "Conflicts found.".red().bold(),
            resp.potential_conflicts.len(),
        );
        for c in &resp.potential_conflicts {
            println!(
                "  {} {} :: {}",
                "conflict:".red(),
                c.file_path,
                c.symbol_name,
            );
            println!("    ours:   {}", c.our_change);
            println!("    theirs: {}", c.their_change);
        }
    } else {
        println!("{}", "No conflicts.".green().bold());
    }

    println!(
        "\nSummary: {} file(s) modified, {} symbol(s) changed",
        resp.files_modified, resp.symbols_changed,
    );

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Create a raw gRPC client by building a tonic Channel, then wrapping it
/// in the generated `AgentServiceClient`.  This avoids name-collision with
/// the `connect` RPC method on the client.
async fn grpc_client(addr: &str) -> Result<AgentServiceClient<Channel>> {
    let channel = Channel::from_shared(addr.to_string())
        .context("invalid server address")?
        .connect()
        .await
        .context("failed to connect \u{2014} is dk-server running?")?;
    Ok(AgentServiceClient::new(channel))
}

fn resolve_token(token: Option<String>) -> Result<String> {
    match token {
        Some(t) => Ok(t),
        None => {
            let config = Config::load()?;
            Ok(config
                .server
                .token
                .unwrap_or_else(|| "dk-alpha-token".to_string()))
        }
    }
}
