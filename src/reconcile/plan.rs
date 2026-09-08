use super::{Domain, Operation};
use anyhow::{Context, Result, bail};
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
        if schema_version != 4 {
            bail!("unsupported plan schema {schema_version}; run plan again")
        }
        if let Some(operations) = value
            .get("operations")
            .and_then(serde_json::Value::as_array)
        {
            for (index, operation) in operations.iter().enumerate() {
                if let Some(domain) = operation.get("domain") {
                    serde_json::from_value::<Domain>(domain.clone()).with_context(|| {
                        format!("invalid plan field operations[{index}].domain")
                    })?;
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_deserialization_reports_invalid_domain_field() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 4,
            "created_at": "2026-09-05T00:00:00Z",
            "capture_id": "capture-1",
            "target": "https://pve.test:8006",
            "pbs_target": "https://pbs.test:8007",
            "operations": [{
                "action": "api-mutation",
                "target": "pve",
                "method": "put",
                "domain": "Guest",
                "resource": "lxc/101",
                "endpoint": "/nodes/pve/lxc/101/config",
                "changes": {},
                "digest": null
            }],
            "blockers": [],
            "plan_sha256": "digest"
        }))
        .unwrap();

        let error = Plan::from_slice(&bytes).unwrap_err();

        assert!(format!("{error:#}").contains("operations[0].domain"));
        assert!(format!("{error:#}").contains("unknown operation domain `Guest`"));
    }

    #[test]
    fn old_plan_schema_requires_replanning() {
        let bytes = br#"{"schema_version":3}"#;

        let error = Plan::from_slice(bytes).unwrap_err();

        assert_eq!(
            error.to_string(),
            "unsupported plan schema 3; run plan again"
        );
    }
}
