use anyhow::Result;

use crate::config::Config;

pub fn run() -> Result<()> {
    let mut config = Config::load().unwrap_or_default();
    config.server.url = None;
    config.server.token = None;
    config.save()?;
    println!("Logged out.");
    Ok(())
}
