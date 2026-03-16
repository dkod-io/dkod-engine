use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

use crate::config::Config;

pub struct Client {
    http: reqwest::blocking::Client,
    base_url: String,
    token: String,
}

impl Client {
    pub fn from_config() -> Result<Self> {
        let config = Config::load()?;
        let (url, token) = config.require_auth()?;
        Ok(Self {
            http: reqwest::blocking::Client::new(),
            base_url: url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        })
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let res = self.http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .context("request failed")?;
        Self::handle_response(res)
    }

    pub fn post<T: DeserializeOwned>(&self, path: &str, body: &impl serde::Serialize) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let res = self.http
            .post(&url)
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .context("request failed")?;
        Self::handle_response(res)
    }

    pub fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let res = self.http
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .context("request failed")?;
        Self::handle_response(res)
    }

    pub fn delete(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let res = self.http
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .context("request failed")?;
        Self::handle_response(res)
    }

    /// Send a login request (no auth header needed).
    pub fn login(base_url: &str, email: &str, password: &str) -> Result<serde_json::Value> {
        let http = reqwest::blocking::Client::new();
        let url = format!("{}/auth/login", base_url.trim_end_matches('/'));
        let res = http
            .post(&url)
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .context("login request failed")?;
        Self::handle_response(res)
    }

    fn handle_response<T: DeserializeOwned>(res: reqwest::blocking::Response) -> Result<T> {
        let status = res.status();
        if !status.is_success() {
            let body = res.text().unwrap_or_default();
            anyhow::bail!("server returned {}: {}", status.as_u16(), body);
        }
        res.json::<T>().context("failed to parse response")
    }
}
