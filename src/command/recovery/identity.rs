use super::plan::RecoveryPlan;
use crate::client::RemoteHost;
use anyhow::{Result, bail};

pub(super) fn verify(ssh: &dyn RemoteHost, plan: &RecoveryPlan) -> Result<()> {
    let fingerprint = ssh.host_key_fingerprint()?;
    if fingerprint != plan.expected_host_key_sha256 {
        bail!(
            "recovery host key mismatch: expected {}, negotiated {}",
            plan.expected_host_key_sha256,
            fingerprint
        )
    }
    let actual = normalize_hostname(ssh.run("hostname")?.trim());
    let expected = normalize_hostname(&plan.expected_hostname);
    if actual != expected {
        bail!(
            "recovery host identity mismatch: expected hostname {}, got {}",
            plan.expected_hostname,
            actual
        )
    }
    Ok(())
}

fn normalize_hostname(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostnames_are_normalized_for_identity_checks() {
        assert_eq!(
            normalize_hostname(" PVE.EXAMPLE.TEST. "),
            "pve.example.test"
        );
        assert_eq!(normalize_hostname("[2001:DB8::1]"), "2001:db8::1");
    }
}
