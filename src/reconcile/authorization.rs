use super::{ApiTarget, Operation, Plan};
use anyhow::{Result, bail};

pub(super) fn verify(plan: &Plan) -> Result<()> {
    if plan.schema_version != 3 {
        bail!(
            "unsupported plan schema {}; run plan again",
            plan.schema_version
        )
    }
    crate::utility::plan_envelope::verify(plan, "plan file integrity check failed")?;
    for operation in &plan.operations {
        verify_operation(operation)?;
    }
    Ok(())
}

fn verify_operation(operation: &Operation) -> Result<()> {
    match operation {
        Operation::ApiMutation {
            target,
            domain,
            resource,
            endpoint,
            environment_changes,
            ..
        } => {
            super::change::validate_operation(*domain, *target, resource)?;
            if !endpoint.is_valid() {
                bail!("invalid API path {endpoint}")
            }
            if let Some(secret) = environment_changes.values().find(|value| !value.is_valid()) {
                bail!("invalid secret environment variable {secret}")
            }
        },
        Operation::GrowDisk {
            domain,
            resource,
            endpoint,
            disk,
            ..
        } => {
            super::change::validate_operation(*domain, ApiTarget::Pve, resource)?;
            if !endpoint.is_valid() {
                bail!("invalid API path {endpoint}")
            }
            if !disk.is_valid() {
                bail!("invalid guest disk identifier {disk}")
            }
        },
        Operation::WriteFile {
            domain,
            resource,
            target,
            ..
        }
        | Operation::DeleteFile {
            domain,
            resource,
            target,
            ..
        } => {
            super::change::validate_operation(*domain, ApiTarget::Pve, resource)?;
            if !target.matches(*domain, resource) {
                bail!(
                    "managed file {} is incompatible with domain {domain} and resource {resource}",
                    target.path()
                )
            }
        },
    }
    Ok(())
}
