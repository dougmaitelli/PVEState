use super::{ApiMethod, ApiTarget, Domain, MutationEndpointFamily, Operation, Plan, ResourceId};
use anyhow::{Result, bail};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

pub(super) fn verify(plan: &Plan) -> Result<()> {
    if plan.schema_version != 4 {
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
            method,
            domain,
            resource,
            endpoint,
            changes,
            environment_changes,
            digest,
            ..
        } => {
            super::change::validate_operation(*domain, *target, resource)?;
            verify_api_mutation(*target, *method, *domain, resource, endpoint, changes)?;
            if let Some(secret) = environment_changes.values().find(|value| !value.is_valid()) {
                bail!("invalid secret environment variable {secret}")
            }
            if !environment_changes.is_empty()
                && !matches!(
                    endpoint.family(),
                    Some(MutationEndpointFamily::PbsCollection { kind: "s3" })
                )
            {
                bail!("secret parameters are not allowed for API endpoint {endpoint}")
            }
            if environment_changes
                .keys()
                .any(|key| !matches!(key.as_str(), "access-key" | "secret-key"))
            {
                bail!("unauthorized secret parameter for API endpoint {endpoint}")
            }
            for (parameter, secret) in environment_changes {
                let expected = match parameter.as_str() {
                    "access-key" => crate::settings::env::PBS_APPLY_S3_ACCESS_KEY,
                    "secret-key" => crate::settings::env::PBS_APPLY_S3_SECRET_KEY,
                    _ => continue,
                };
                if secret.as_str() != expected {
                    bail!("secret {secret} is not authorized for parameter `{parameter}`")
                }
            }
            if digest.is_some()
                && !matches!(
                    endpoint.family(),
                    Some(MutationEndpointFamily::GuestConfig { .. })
                )
            {
                bail!("digest is not authorized for API endpoint {endpoint}")
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

fn verify_api_mutation(
    target: ApiTarget,
    method: ApiMethod,
    domain: Domain,
    resource: &ResourceId,
    endpoint: &super::MutationEndpoint,
    changes: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let family = endpoint
        .family()
        .ok_or_else(|| anyhow::anyhow!("API mutation endpoint is not managed `{endpoint}`"))?;
    let allows_lxc_options = matches!(
        family,
        MutationEndpointFamily::GuestConfig { kind: "lxc", .. }
    );
    let allowed = match family {
        MutationEndpointFamily::GuestConfig { node, kind, vmid } => {
            let ResourceId::Guest(guest) = resource else {
                bail!("guest endpoint {endpoint} requires a guest resource")
            };
            if target != ApiTarget::Pve
                || domain != Domain::Guest
                || method != ApiMethod::Put
                || guest.kind.api_name() != kind
                || guest.vmid.to_string() != vmid
                || !valid_identity(node)
            {
                bail!("API endpoint {endpoint} does not match its guest operation")
            }
            guest_parameters
        },
        MutationEndpointFamily::NodeDns { node } => {
            if target != ApiTarget::Pve
                || domain != Domain::Dns
                || method != ApiMethod::Put
                || resource.to_string() != node
                || !valid_identity(node)
            {
                bail!("API endpoint {endpoint} does not match its DNS operation")
            }
            dns_parameters
        },
        MutationEndpointFamily::PveBackupCollection => {
            if target != ApiTarget::Pve || domain != Domain::Backup || method != ApiMethod::Post {
                bail!("API endpoint {endpoint} does not match its backup operation")
            }
            let id = resource.to_string();
            if changes.get("id").map(String::as_str) != id.strip_prefix("pve/") {
                bail!("PVE backup collection operation has a mismatched resource identity")
            }
            pve_backup_parameters
        },
        MutationEndpointFamily::PveBackupJob { id } => {
            verify_named_resource(target, domain, resource, endpoint, "pve", id)?;
            if method == ApiMethod::Post {
                bail!("PVE backup item endpoints do not support POST")
            }
            pve_backup_parameters
        },
        MutationEndpointFamily::PbsCollection { kind } => {
            if target != ApiTarget::Pbs || domain != Domain::Pbs || method != ApiMethod::Post {
                bail!("API endpoint {endpoint} does not match its PBS operation")
            }
            let id_key = if kind == "datastore" { "name" } else { "id" };
            let id = changes.get(id_key).map(String::as_str).unwrap_or_default();
            if resource.to_string() != format!("{kind}/{id}") || id.is_empty() {
                bail!("PBS collection operation has a mismatched resource identity")
            }
            pbs_parameters(kind)?
        },
        MutationEndpointFamily::PbsResource { kind, id } => {
            verify_named_resource(target, domain, resource, endpoint, kind, id)?;
            if method == ApiMethod::Post {
                bail!("PBS item endpoints do not support POST")
            }
            pbs_parameters(kind)?
        },
    };
    for key in changes.keys() {
        if !allowed(key) && !(allows_lxc_options && lxc_option_parameter(key)) {
            bail!("parameter `{key}` is not authorized for API endpoint {endpoint}")
        }
    }
    Ok(())
}

type ParameterPolicy = fn(&str) -> bool;

fn verify_named_resource(
    target: ApiTarget,
    domain: Domain,
    resource: &ResourceId,
    endpoint: &super::MutationEndpoint,
    prefix: &str,
    encoded_id: &str,
) -> Result<()> {
    let resource = resource.to_string();
    let Some(id) = resource.strip_prefix(&format!("{prefix}/")) else {
        bail!("API endpoint {endpoint} has an incompatible resource identity")
    };
    let (expected_target, expected_domain) = if prefix == "pve" {
        (ApiTarget::Pve, Domain::Backup)
    } else {
        (ApiTarget::Pbs, Domain::Pbs)
    };
    if target != expected_target
        || domain != expected_domain
        || utf8_percent_encode(id, NON_ALPHANUMERIC).to_string() != encoded_id
    {
        bail!("API endpoint {endpoint} does not match its resource operation")
    }
    Ok(())
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn guest_parameters(key: &str) -> bool {
    matches!(
        key,
        "hostname"
            | "name"
            | "cores"
            | "memory"
            | "swap"
            | "onboot"
            | "startup"
            | "machine"
            | "bios"
            | "sockets"
            | "cpu"
            | "agent"
            | "unprivileged"
            | "ostype"
            | "rootfs"
            | "delete"
    ) || [
        "net", "usb", "mp", "scsi", "virtio", "sata", "ide", "efidisk",
    ]
    .iter()
    .any(|prefix| {
        key.strip_prefix(prefix)
            .is_some_and(|suffix| suffix.parse::<u8>().is_ok())
    })
}

fn lxc_option_parameter(key: &str) -> bool {
    match crate::model::LxcConfigField::classify(key) {
        crate::model::LxcConfigField::Additional(name) => name.as_str() == key,
        _ => false,
    }
}

fn dns_parameters(key: &str) -> bool {
    key == "search"
        || key == "delete"
        || key
            .strip_prefix("dns")
            .is_some_and(|suffix| suffix.parse::<u8>().is_ok_and(|index| index > 0))
}

fn pve_backup_parameters(key: &str) -> bool {
    matches!(
        key,
        "id" | "node" | "storage" | "schedule" | "mode" | "vmid" | "prune-backups"
    )
}

fn pbs_parameters(kind: &str) -> Result<ParameterPolicy> {
    match kind {
        "datastore" => Ok(|key| matches!(key, "name" | "path" | "backend" | "gc-schedule")),
        "s3" => Ok(|key| matches!(key, "id" | "endpoint" | "region")),
        "prune" => Ok(|key| matches!(key, "id" | "store" | "schedule" | "keep-last")),
        "verify" => Ok(|key| {
            matches!(
                key,
                "id" | "store" | "schedule" | "ignore-verified" | "outdated-after"
            )
        }),
        "sync" => Ok(|key| {
            matches!(
                key,
                "id" | "store"
                    | "remote-store"
                    | "remove-vanished"
                    | "sync-direction"
                    | "remote"
                    | "schedule"
            )
        }),
        _ => bail!("unsupported PBS resource kind {kind}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn mutation(endpoint: &str) -> Operation {
        Operation::ApiMutation {
            target: ApiTarget::Pve,
            method: ApiMethod::Put,
            domain: Domain::Guest,
            resource: ResourceId::parse("lxc/101"),
            endpoint: endpoint.into(),
            changes: BTreeMap::new(),
            before_values: BTreeMap::new(),
            environment_changes: BTreeMap::new(),
            digest: None,
        }
    }

    #[test]
    fn arbitrary_api_paths_are_rejected() {
        let error = verify_operation(&mutation("/access/users")).unwrap_err();

        assert!(error.to_string().contains("not managed"));
        assert!(
            serde_json::from_str::<super::super::MutationEndpoint>("\"/access/users\"").is_err()
        );
    }

    #[test]
    fn guest_endpoint_identity_must_match_resource() {
        let error = verify_operation(&mutation("/nodes/pve/lxc/102/config")).unwrap_err();

        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn endpoint_specific_parameter_policy_is_enforced() {
        let mut operation = mutation("/nodes/pve/lxc/101/config");
        let Operation::ApiMutation { changes, .. } = &mut operation else {
            unreachable!()
        };
        changes.insert("password".into(), "not-authorized".into());

        let error = verify_operation(&operation).unwrap_err();

        assert!(error.to_string().contains("not authorized"));
    }
}
