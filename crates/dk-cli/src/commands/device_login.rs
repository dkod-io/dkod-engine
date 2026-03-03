use anyhow::Result;
use crate::auth;
use crate::output::Output;

pub async fn run(out: Output, server: &str) -> Result<()> {
    let api_base = auth::api_base_from_grpc(server);

    let token = auth::run_device_flow(&api_base).await?;
    if out.is_json() {
        out.print_json(&serde_json::json!({
            "status": "authenticated",
            "token_cached": true,
        }));
    }
    let _ = token;
    Ok(())
}
