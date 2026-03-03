use anyhow::Result;
use colored::Colorize;
use dk_protocol::SubmitRequest;

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output, message: &str) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let resp = client
        .submit(SubmitRequest {
            session_id: state.session_id,
            intent: message.to_string(),
            changes: vec![],
            changeset_id: state.changeset_id,
        })
        .await?
        .into_inner();

    let status = format!("{:?}", resp.status());

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "changeset_id": resp.changeset_id,
            "status": status,
            "errors": resp.errors.iter().map(|e| {
                serde_json::json!({"file": e.file_path, "message": e.message})
            }).collect::<Vec<_>>(),
        }));
    } else if resp.errors.is_empty() {
        println!("{} changeset {}", "Submitted.".green().bold(), resp.changeset_id.dimmed());
    } else {
        println!("{} changeset {}", "Submit had errors.".red().bold(), resp.changeset_id.dimmed());
        for err in &resp.errors {
            let loc = err.file_path.as_deref().unwrap_or("?");
            println!("  {} {}: {}", "error:".red(), loc, err.message);
        }
    }

    Ok(())
}
