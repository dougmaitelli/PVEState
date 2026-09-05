use crate::{
    config::LocalState,
    reconcile::{Domain, ManagedFile, PlanBuilder},
    resource::{native, native_paths},
};
use anyhow::Result;
use std::fs;

pub(crate) fn plan(local: &LocalState, builder: &mut PlanBuilder) -> Result<()> {
    if let Ok(captured) = fs::read_to_string(local.observed().join(native_paths::NETWORK_ARTIFACT))
    {
        builder
            .blockers()
            .extend(safety_blockers(&captured, &local.network)?);
    }
    crate::resource::file_plan::operation(
        local,
        (Domain::Network, &local.guests.node),
        native_paths::NETWORK_ARTIFACT,
        ManagedFile::NetworkInterfaces,
        super::render::render(&local.network),
        builder.operations(),
    )
}

fn safety_blockers(content: &str, local: &super::Network) -> Result<Vec<String>> {
    let parsed = native::parse_network(content, local)?;
    Ok(parsed
        .unmodeled
        .iter()
        .map(|directive| {
            format!(
                "network configuration cannot be represented safely: {}",
                directive.diagnostic()
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_native_syntax_becomes_a_blocker() {
        let network: super::super::Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let blockers = safety_blockers(
            "iface vmbr0 inet manual\n    bridge-ports none\n    bridge-vlan-aware yes\n",
            &network,
        )
        .unwrap();

        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("cannot be represented safely"));
        assert!(blockers[0].contains("bridge-vlan-aware yes"));
    }
}
