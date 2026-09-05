use super::{PveClient, transport::JsonApiClient};
use crate::settings::{ApiCredential, PveSettings};
use anyhow::Result;
use reqwest::Method;
use serde_json::Value;
use std::collections::BTreeMap;
pub struct Pve {
    transport: JsonApiClient,
}

impl Pve {
    pub fn discovery(settings: &PveSettings) -> Result<Self> {
        Self::new(settings, settings.discovery_credential()?)
    }

    pub fn mutation(settings: &PveSettings) -> Result<Self> {
        Self::new(settings, settings.mutation_credential()?)
    }

    fn new(settings: &PveSettings, credential: &ApiCredential) -> Result<Self> {
        Ok(Self {
            transport: JsonApiClient::new(
                &settings.endpoint,
                settings.verify_tls,
                settings.ca_file.as_deref(),
                &format!("PVEAPIToken={}={}", credential.id, credential.secret),
            )?,
        })
    }

    pub fn endpoint(&self) -> &str {
        self.transport.endpoint()
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.transport.get_data(path)
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
        self.transport.form(method, path, data)
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
