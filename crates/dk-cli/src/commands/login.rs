use anyhow::{Context, Result};

use crate::client::Client;
use crate::config::Config;

pub fn run(url: String) -> Result<()> {
    let url = url.trim_end_matches('/').to_string();

    // Prompt for credentials
    eprint!("Email: ");
    let mut email = String::new();
    std::io::stdin().read_line(&mut email)?;
    let email = email.trim().to_string();

    let password = rpassword::prompt_password("Password: ")
        .context("failed to read password")?;

    let response: serde_json::Value = Client::login(&url, &email, &password)?;
    let token = response["token"]
        .as_str()
        .context("server did not return a token")?;

    let mut config = Config::load().unwrap_or_default();
    config.server.url = Some(url.clone());
    config.server.token = Some(token.to_string());
    config.save()?;

    println!("Logged in to {} as {}", url, email);
    Ok(())
}
