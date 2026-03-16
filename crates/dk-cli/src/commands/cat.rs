use anyhow::Result;
use colored::Colorize;
use dk_protocol::FileReadRequest;

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output, path: &str) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let resp = client
        .file_read(FileReadRequest {
            session_id: state.session_id,
            path: path.to_string(),
        })
        .await?
        .into_inner();

    if out.is_json() {
        let content_str = String::from_utf8_lossy(&resp.content);
        out.print_json(&serde_json::json!({
            "path": path,
            "content": content_str,
            "hash": resp.hash,
            "modified_in_session": resp.modified_in_session,
        }));
    } else {
        match String::from_utf8(resp.content.clone()) {
            Ok(text) => print!("{}", text),
            Err(_) => {
                eprintln!("{} {} ({} bytes, binary)", "File:".green().bold(), path, resp.content.len());
            }
        }
    }

    Ok(())
}
