use anyhow::Result;

use crate::client::Client;

pub fn run(query: String, repo: String, limit: Option<usize>) -> Result<()> {
    let client = Client::from_config()?;
    let limit = limit.unwrap_or(10);
    let path = format!("/repos/{}/search?q={}&limit={}", repo, urlencoding::encode(&query), limit);
    let resp: serde_json::Value = client.get(&path)?;

    let symbols = resp["symbols"].as_array();
    if symbols.is_none_or(|s| s.is_empty()) {
        println!("No results.");
        return Ok(());
    }

    println!("{:>3} | {:>5} | {:<10} | {:<35} | Symbol", "#", "Score", "Kind", "File");
    println!("{}", "-".repeat(90));

    for (i, sym) in symbols.unwrap().iter().enumerate() {
        let score = sym["score"].as_f64().unwrap_or(0.0);
        let kind = sym["kind"].as_str().unwrap_or("?");
        let file = sym["file_path"].as_str().unwrap_or("?");
        let name = sym["qualified_name"].as_str().unwrap_or("?");
        println!("{:>3} | {:>5.2} | {:<10} | {:<35} | {}", i + 1, score, kind, file, name);
    }

    Ok(())
}
