use anyhow::Result;

use crate::client::Client;

pub fn run(query: String, repo: String, max_tokens: Option<usize>) -> Result<()> {
    let client = Client::from_config()?;
    let tokens = max_tokens.unwrap_or(4000);
    let path = format!("/repos/{}/context?q={}&max_tokens={}", repo, urlencoding::encode(&query), tokens);
    let resp: serde_json::Value = client.get(&path)?;

    let symbols = resp["symbols"].as_array();
    if symbols.is_none_or(|s| s.is_empty()) {
        println!("No results.");
        return Ok(());
    }

    for sym in symbols.unwrap() {
        let file = sym["file_path"].as_str().unwrap_or("?");
        let kind = sym["kind"].as_str().unwrap_or("?");
        let name = sym["name"].as_str().unwrap_or("?");
        let snippet = sym["snippet"].as_str().unwrap_or("");

        println!("-- {} ({} {}) --", file, kind, name);
        println!("{}", snippet);
        println!();
    }

    let total = resp["total_tokens"].as_u64().unwrap_or(0);
    let count = symbols.unwrap().len();
    println!("[{} symbols, {} tokens]", count, total);

    Ok(())
}
