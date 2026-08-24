use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::BTreeMap;
pub struct Pve {
    base: String,
    token: String,
    http: Client,
}
impl Pve {
    pub fn discovery() -> Result<Self> {
        Self::from_env("PVE_ENDPOINT", "PVE_API_TOKEN_ID", "PVE_API_TOKEN_SECRET")
    }
    pub fn mutation() -> Result<Self> {
        Self::from_env(
            "PVE_APPLY_ENDPOINT",
            "PVE_APPLY_API_TOKEN_ID",
            "PVE_APPLY_API_TOKEN_SECRET",
        )
    }
    fn from_env(e: &str, i: &str, s: &str) -> Result<Self> {
        let base = std::env::var(e).with_context(|| e.to_string())?;
        let id = std::env::var(i).with_context(|| i.to_string())?;
        let secret = std::env::var(s).with_context(|| s.to_string())?;
        let http = Client::builder()
            .danger_accept_invalid_certs(std::env::var("PVE_VERIFY_TLS").as_deref() == Ok("false"))
            .build()?;
        Ok(Self {
            base: format!("{}/api2/json", base.trim_end_matches('/')),
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
