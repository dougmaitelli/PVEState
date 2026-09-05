use super::Operation;
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub(crate) schema_version: u8,
    pub(crate) created_at: DateTime<Utc>,
    #[serde(default)]
    pub(crate) capture_id: String,
    pub(crate) target: String,
    pub(crate) pbs_target: String,
    pub(crate) operations: Vec<Operation>,
    pub(crate) blockers: Vec<String>,
    pub(crate) plan_sha256: String,
}

impl Plan {
    pub(crate) fn from_slice(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        if schema_version != 3 {
            bail!("unsupported plan schema {schema_version}; run plan again")
        }
        Ok(serde_json::from_value(value)?)
    }

    #[cfg(test)]
    pub(crate) fn calculate_hash(&self) -> Result<String> {
        let mut signed = self.clone();
        crate::utility::plan_envelope::sign(&mut signed)?;
        Ok(signed.plan_sha256)
    }

    pub(crate) fn verify(&self) -> Result<()> {
        super::authorization::verify(self)
    }
}

impl crate::utility::plan_envelope::PlanEnvelope for Plan {
    fn integrity(&self) -> &str {
        &self.plan_sha256
    }

    fn set_integrity(&mut self, value: String) {
        self.plan_sha256 = value;
    }
}
