//! Thin wrapper around [`clickhouse::Client`] that centralises URL/user/db
//! parsing from environment variables. Cheaply cloneable (it wraps a
//! connection-reuse `hyper` pool internally).

use anyhow::{Context, Result};
use clickhouse::Client;

/// Environment variable holding the ClickHouse HTTP URL. When unset, the
/// sink runs in no-op mode and no client is created.
pub const URL_ENV: &str = "CLICKHOUSE_URL";
/// Optional credentials and default database.
pub const USER_ENV: &str = "CLICKHOUSE_USER";
pub const PASSWORD_ENV: &str = "CLICKHOUSE_PASSWORD";
pub const DATABASE_ENV: &str = "CLICKHOUSE_DATABASE";

#[derive(Clone, Debug)]
pub struct AnalyticsConfig {
    pub url: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
}

impl AnalyticsConfig {
    /// Build the config from the `CLICKHOUSE_*` env vars. Returns `None`
    /// when `CLICKHOUSE_URL` is unset, which keeps the sink in no-op mode.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var(URL_ENV).ok()?;
        let user = std::env::var(USER_ENV).ok();
        let password = std::env::var(PASSWORD_ENV).ok();
        let database = std::env::var(DATABASE_ENV).ok();
        Some(Self {
            url,
            user,
            password,
            database,
        })
    }
}

#[derive(Clone)]
pub struct AnalyticsClient {
    inner: Client,
    database: Option<String>,
}

impl AnalyticsClient {
    /// Connect using the supplied config. No network I/O happens until the
    /// first query is executed.
    pub fn new(cfg: &AnalyticsConfig) -> Result<Self> {
        let mut client = Client::default().with_url(&cfg.url);
        if let Some(user) = &cfg.user {
            client = client.with_user(user);
        }
        if let Some(password) = &cfg.password {
            client = client.with_password(password);
        }
        if let Some(database) = &cfg.database {
            client = client.with_database(database);
        }
        Ok(Self {
            inner: client,
            database: cfg.database.clone(),
        })
    }

    pub fn from_env() -> Result<Option<Self>> {
        match AnalyticsConfig::from_env() {
            Some(cfg) => Self::new(&cfg).map(Some),
            None => Ok(None),
        }
    }

    pub fn inner(&self) -> &Client {
        &self.inner
    }

    pub fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// One-shot ping. Useful from `dk analytics migrate` to fail fast when
    /// ClickHouse is unreachable before running DDL.
    pub async fn ping(&self) -> Result<()> {
        self.inner
            .query("SELECT 1")
            .execute()
            .await
            .context("ClickHouse SELECT 1 probe failed")
    }
}
