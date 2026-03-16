use anyhow::Result;

use crate::client::Client;

pub fn run(repo: String) -> Result<()> {
    let client = Client::from_config()?;
    let resp: serde_json::Value = client.post_empty(&format!("/repos/{}/index", repo))?;

    let symbols = resp["symbols_indexed"].as_u64().unwrap_or(0);
    let vectors = resp["vectors_stored"].as_u64().unwrap_or(0);
    let ms = resp["duration_ms"].as_u64().unwrap_or(0);

    println!("Indexed {} symbols ({} vectors) in {:.1}s", symbols, vectors, ms as f64 / 1000.0);
    Ok(())
}
