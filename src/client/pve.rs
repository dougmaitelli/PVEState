use crate::config::env;
use anyhow::{Context, Result};
use reqwest::{Certificate, blocking::Client};
use serde_json::Value;
use std::{collections::BTreeMap, fs, time::Duration};
pub struct Pve {
    base: String,
    token: String,
    http: Client,
}

impl Pve {
    pub fn discovery() -> Result<Self> {
        Self::from_env("PVE_API_TOKEN_ID", "PVE_API_TOKEN_SECRET")
    }

    pub fn mutation() -> Result<Self> {
        Self::from_env("PVE_APPLY_API_TOKEN_ID", "PVE_APPLY_API_TOKEN_SECRET")
    }

    fn from_env(i: &str, s: &str) -> Result<Self> {
        let host = env("PVE_HOST", None)?;
        let scheme = env("PVE_API_SCHEME", Some("https"))?;
        let port = env("PVE_API_PORT", Some("8006"))?;
        let id = std::env::var(i).with_context(|| i.to_string())?;
        let secret = std::env::var(s).with_context(|| s.to_string())?;
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(std::env::var("PVE_VERIFY_TLS").as_deref() == Ok("false"));
        if let Ok(path) = std::env::var("PVE_CA_FILE") {
            let pem = fs::read(&path).with_context(|| format!("read PVE CA file {path}"))?;
            builder = builder.add_root_certificate(Certificate::from_pem(&pem)?);
        }
        let http = builder.build()?;
        Ok(Self {
            base: format!("{scheme}://{host}:{port}/api2/json"),
            token: format!("PVEAPIToken={id}={secret}"),
            http,
        })
    }

    pub fn endpoint(&self) -> &str {
        self.base.trim_end_matches("/api2/json")
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        let v: Value = self
            .http
            .get(format!("{}{}", self.base, path))
            .header("Authorization", &self.token)
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                concat!("pvestate/", env!("CARGO_PKG_VERSION")),
            )
            .send()?
            .error_for_status()?
            .json()?;
        Ok(v["data"].clone())
    }

    pub fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.http
            .put(format!("{}{}", self.base, path))
            .header("Authorization", &self.token)
            .form(data)
            .send()?
            .error_for_status()?;
        Ok(())
    }
}
