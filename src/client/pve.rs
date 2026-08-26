use super::PveClient;
use crate::settings::{ApiCredential, PveSettings};
use anyhow::{Context, Result};
use reqwest::{Certificate, Method, blocking::Client};
use serde_json::Value;
use std::{collections::BTreeMap, fs, time::Duration};
pub struct Pve {
    base: String,
    token: String,
    http: Client,
}

impl Pve {
    pub fn discovery(settings: &PveSettings) -> Result<Self> {
        Self::new(settings, settings.discovery_credential()?)
    }

    pub fn mutation(settings: &PveSettings) -> Result<Self> {
        Self::new(settings, settings.mutation_credential()?)
    }

    fn new(settings: &PveSettings, credential: &ApiCredential) -> Result<Self> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(!settings.verify_tls);
        if let Some(path) = &settings.ca_file {
            let pem =
                fs::read(path).with_context(|| format!("read PVE CA file {}", path.display()))?;
            builder = builder.add_root_certificate(Certificate::from_pem(&pem)?);
        }
        let http = builder.build()?;
        Ok(Self {
            base: format!(
                "{}/api2/json",
                settings.endpoint.as_str().trim_end_matches('/')
            ),
            token: format!("PVEAPIToken={}={}", credential.id, credential.secret),
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
        self.mutate(Method::PUT, path, data)
    }

    pub fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::POST, path, data)
    }

    pub fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::DELETE, path, data)
    }

    fn mutate(&self, method: Method, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.http
            .request(method, format!("{}{}", self.base, path))
            .header("Authorization", &self.token)
            .form(data)
            .send()?
            .error_for_status()?;
        Ok(())
    }
}

impl PveClient for Pve {
    fn endpoint(&self) -> &str {
        self.endpoint()
    }
    fn get(&self, path: &str) -> Result<Value> {
        self.get(path)
    }
    fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.put(path, data)
    }
    fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.post(path, data)
    }
    fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.delete(path, data)
    }
}
