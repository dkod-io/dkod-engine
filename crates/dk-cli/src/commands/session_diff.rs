use anyhow::Result;
use colored::Colorize;
use dk_protocol::FileListRequest;

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let resp = client
        .file_list(FileListRequest {
            session_id: state.session_id,
            prefix: None,
            only_modified: true,
        })
        .await?
        .into_inner();

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "modified_files": resp.files.iter().map(|f| &f.path).collect::<Vec<_>>(),
            "count": resp.files.len(),
        }));
    } else if resp.files.is_empty() {
        println!("No pending changes.");
    } else {
        println!("{}", "Modified files:".bold());
        for entry in &resp.files {
            println!("  {} {}", "M".yellow(), entry.path);
        }
        println!("\n{} file(s) modified", resp.files.len());
    }

    Ok(())
}
