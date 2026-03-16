use anyhow::Result;

use crate::client::Client;

pub fn create(name: String) -> Result<()> {
    let client = Client::from_config()?;
    let _: serde_json::Value = client.post("/repos", &serde_json::json!({ "name": name }))?;
    println!("Created repository '{}'", name);
    Ok(())
}

pub fn list() -> Result<()> {
    let client = Client::from_config()?;
    let repos: Vec<serde_json::Value> = client.get("/repos")?;

    if repos.is_empty() {
        println!("No repositories.");
        return Ok(());
    }

    println!("{:<30} {:<20}", "Name", "Created");
    println!("{}", "-".repeat(50));
    for repo in repos {
        let name = repo["name"].as_str().unwrap_or("?");
        let created = repo["created_at"].as_str().unwrap_or("?");
        println!("{:<30} {:<20}", name, created.get(..10).unwrap_or(created));
    }
    Ok(())
}

pub fn delete(name: String) -> Result<()> {
    let client = Client::from_config()?;
    client.delete(&format!("/repos/{}", name))?;
    println!("Deleted repository '{}'", name);
    Ok(())
}
