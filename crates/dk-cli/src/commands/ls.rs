use anyhow::Result;
use colored::Colorize;
use dk_protocol::FileListRequest;

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output, prefix: Option<&str>, only_modified: bool) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let resp = client
        .file_list(FileListRequest {
            session_id: state.session_id,
            prefix: prefix.map(|s| s.to_string()),
            only_modified,
        })
        .await?
        .into_inner();

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "files": resp.files.iter().map(|f| {
                serde_json::json!({"path": f.path, "modified": f.modified_in_session})
            }).collect::<Vec<_>>(),
            "total": resp.files.len(),
        }));
    } else {
        if resp.files.is_empty() {
            println!("No files found.");
            return Ok(());
        }
        for entry in &resp.files {
            let marker = if entry.modified_in_session {
                "M ".yellow().to_string()
            } else {
                "  ".to_string()
            };
            println!("{}{}", marker, entry.path);
        }
        println!("\n{} file(s)", resp.files.len());
    }

    Ok(())
}
