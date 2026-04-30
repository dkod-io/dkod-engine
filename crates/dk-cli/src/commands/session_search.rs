use anyhow::Result;
use colored::Colorize;
use dk_protocol::{ContextDepth, ContextRequest};

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output, query: &str, depth: &str, max_tokens: u32) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let proto_depth = match depth {
        "signatures" => ContextDepth::Signatures as i32,
        "call_graph" => ContextDepth::CallGraph as i32,
        _ => ContextDepth::Full as i32,
    };

    let resp = client
        .context(ContextRequest {
            session_id: state.session_id,
            query: query.to_string(),
            depth: proto_depth,
            include_tests: false,
            include_dependencies: false,
            max_tokens,
        })
        .await?
        .into_inner();

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "symbols": resp.symbols.iter().map(|s| {
                let sym = s.symbol.as_ref();
                serde_json::json!({
                    "kind": sym.map(|s| s.kind.as_str()).unwrap_or("?"),
                    "name": sym.map(|s| s.qualified_name.as_str()).unwrap_or("?"),
                    "file": sym.map(|s| s.file_path.as_str()).unwrap_or("?"),
                    "source": &s.source,
                })
            }).collect::<Vec<_>>(),
            "estimated_tokens": resp.estimated_tokens,
        }));
    } else {
        if resp.symbols.is_empty() {
            println!("No symbols found.");
            return Ok(());
        }
        for (i, s) in resp.symbols.iter().enumerate() {
            let sym = s.symbol.as_ref();
            let kind = sym.map(|s| s.kind.as_str()).unwrap_or("?");
            let name = sym.map(|s| s.qualified_name.as_str()).unwrap_or("?");
            let file = sym.map(|s| s.file_path.as_str()).unwrap_or("?");
            println!(
                "{} {} {}",
                format!("[{}]", i + 1).dimmed(),
                kind.cyan(),
                name.bold()
            );
            println!("  {}", file.dimmed());
            if let Some(src) = &s.source {
                if !src.is_empty() {
                    for line in src.lines().take(5) {
                        println!("    {}", line);
                    }
                    let total = src.lines().count();
                    if total > 5 {
                        println!("    {} ({} more lines)", "...".dimmed(), total - 5);
                    }
                }
            }
            println!();
        }
        println!("Estimated tokens: {}", resp.estimated_tokens);
    }

    Ok(())
}
