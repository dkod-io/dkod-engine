use anyhow::Result;

use crate::config::Config;

pub fn run() -> Result<()> {
    let config = Config::load()?;
    match (&config.server.url, &config.server.token) {
        (Some(url), Some(_)) => println!("Logged in to {}", url),
        _ => println!("Not logged in. Run `dk login <url>` to authenticate."),
    }
    Ok(())
}
