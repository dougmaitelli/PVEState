use super::{PbsClient, transport::JsonApiClient};
use crate::settings::{ApiCredential, PbsSettings};
use anyhow::Result;
use reqwest::Method;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) struct Pbs {
    transport: JsonApiClient,
}

impl Pbs {
    pub(crate) fn discovery(settings: &PbsSettings) -> Result<Self> {
        Self::new(settings, settings.discovery_credential()?)
    }

    pub(crate) fn mutation(settings: &PbsSettings) -> Result<Self> {
        Self::new(settings, settings.mutation_credential()?)
    }

    fn new(settings: &PbsSettings, credential: &ApiCredential) -> Result<Self> {
        Ok(Self {
            transport: JsonApiClient::new(
                &settings.endpoint,
                settings.verify_tls,
                settings.ca_file.as_deref(),
                &format!("PBSAPIToken {}:{}", credential.id, credential.secret),
            )?,
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.transport.endpoint()
    }

    pub(crate) fn get(&self, path: &str) -> Result<Value> {
        self.transport.get_data(path)
    }

    pub(crate) fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::PUT, path, data)
    }

    pub(crate) fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::POST, path, data)
    }

    pub(crate) fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.mutate(Method::DELETE, path, data)
    }

    fn mutate(&self, method: Method, path: &str, data: &BTreeMap<String, String>) -> Result<()> {
        self.transport.form(method, path, data)
    }
}

impl PbsClient for Pbs {
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
