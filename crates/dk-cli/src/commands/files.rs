use std::path::PathBuf;

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::client::Client;

pub fn upload(repo: String, paths: Vec<PathBuf>) -> Result<()> {
    let client = Client::from_config()?;

    let mut files: Vec<serde_json::Value> = Vec::new();
    let skip_dirs = [".git", "node_modules", "target", "__pycache__", ".next", "dist"];

    for base_path in &paths {
        let base = base_path.canonicalize()
            .with_context(|| format!("path not found: {}", base_path.display()))?;

        if base.is_file() {
            if let Some(content) = read_text_file(&base) {
                let name = base.file_name().unwrap_or_default().to_string_lossy().to_string();
                files.push(serde_json::json!({ "path": name, "content": content }));
            }
            continue;
        }

        for entry in WalkDir::new(&base).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !skip_dirs.iter().any(|d| name == *d)
        }) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let full_path = entry.path();
            let relative = full_path.strip_prefix(&base).unwrap_or(full_path);
            if let Some(content) = read_text_file(full_path) {
                files.push(serde_json::json!({
                    "path": relative.to_string_lossy(),
                    "content": content,
                }));
            }
        }
    }

    if files.is_empty() {
        println!("No text files found.");
        return Ok(());
    }

    let total_size: usize = files.iter()
        .map(|f| f["content"].as_str().map_or(0, |s| s.len()))
        .sum();

    let body = serde_json::json!({ "files": files });
    let resp: serde_json::Value = client.post(&format!("/repos/{}/files", repo), &body)?;
    let uploaded = resp["uploaded"].as_u64().unwrap_or(0);

    println!("Uploaded {} files ({:.1} KB)", uploaded, total_size as f64 / 1024.0);
    Ok(())
}

fn read_text_file(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    // Skip binary files (check first 8KB for null bytes)
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}
