use crate::{
    config::LocalState,
    reconcile::{Domain, ManagedFile, PlanBuilder},
    resource::native_paths,
};
use anyhow::Result;
use std::fs;

pub(crate) fn plan(local: &LocalState, builder: &mut PlanBuilder) -> Result<()> {
    for (resource, path) in artifacts(local) {
        if let Ok(content) = fs::read_to_string(local.observed().join(path)) {
            builder
                .blockers()
                .extend(safety_blockers(&content, &resource));
        }
    }

    plan_policy(
        local,
        "cluster",
        native_paths::CLUSTER_FIREWALL_ARTIFACT,
        ManagedFile::ClusterFirewall,
        local.cluster.firewall.as_ref(),
        builder,
    )?;

    for (vmid, policy) in local
        .guests
        .lxcs
        .iter()
        .map(|(id, guest)| (id, &guest.firewall))
        .chain(
            local
                .guests
                .vms
                .iter()
                .map(|(id, guest)| (id, &guest.firewall)),
        )
    {
        plan_policy(
            local,
            &vmid.to_string(),
            &native_paths::guest_firewall_artifact(vmid),
            ManagedFile::GuestFirewall { vmid: *vmid },
            policy.as_ref(),
            builder,
        )?;
    }

    let node = &local.node.node.name;
    plan_policy(
        local,
        &format!("node/{node}"),
        &native_paths::node_firewall_artifact(node),
        ManagedFile::NodeFirewall { node: node.clone() },
        local.node.firewall.as_ref(),
        builder,
    )
}

fn plan_policy(
    local: &LocalState,
    resource: &str,
    artifact: &str,
    target: ManagedFile,
    policy: Option<&super::FirewallPolicy>,
    builder: &mut PlanBuilder,
) -> Result<()> {
    if let Some(policy) = policy {
        crate::resource::file_plan::operation(
            local,
            (Domain::Firewall, resource),
            artifact,
            target,
            super::render::render(policy),
            builder.operations(),
        )
    } else {
        crate::resource::file_plan::deletion(
            local,
            (Domain::Firewall, resource),
            artifact,
            target,
            builder.operations(),
        )
    }
}

fn artifacts(local: &LocalState) -> Vec<(String, String)> {
    let mut artifacts = vec![
        (
            "cluster".into(),
            native_paths::CLUSTER_FIREWALL_ARTIFACT.into(),
        ),
        (
            format!("node/{}", local.guests.node),
            native_paths::node_firewall_artifact(&local.guests.node),
        ),
    ];
    artifacts.extend(
        local
            .guests
            .lxcs
            .keys()
            .chain(local.guests.vms.keys())
            .map(|id| (id.to_string(), native_paths::guest_firewall_artifact(id))),
    );
    artifacts
}

fn safety_blockers(content: &str, resource: &str) -> Vec<String> {
    super::native::parse(content)
        .unmodeled
        .iter()
        .map(|directive| {
            format!(
                "firewall {resource} cannot be represented safely: {}",
                directive.diagnostic()
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_native_flags_become_a_blocker() {
        let blockers = safety_blockers(
            "[RULES]\nIN ACCEPT -p tcp -m conntrack -dport 443\n",
            "cluster",
        );

        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("firewall cluster cannot be represented safely"));
        assert!(blockers[0].contains("-m"));
    }

    #[test]
    fn option_order_is_not_drift_but_rule_order_is() {
        let options_a = "[OPTIONS]\nenable: 1\nlog_level_in: nolog\n";
        let options_b = "[OPTIONS]\nlog_level_in: nolog\nenable: 1\n";
        assert_eq!(
            super::super::render::semantic(options_a),
            super::super::render::semantic(options_b)
        );

        let rules_a = "[RULES]\nIN ACCEPT -dport 22\nIN DROP\n";
        let rules_b = "[RULES]\nIN DROP\nIN ACCEPT -dport 22\n";
        assert_ne!(
            super::super::render::semantic(rules_a),
            super::super::render::semantic(rules_b)
        );
    }

    #[test]
    fn equivalent_rule_flag_order_converges_after_adoption() {
        let captured = "[OPTIONS]\n\nlog_level_in: nolog\nenable: 1\n\n[RULES]\n\nIN ACCEPT -i net1 -p tcp -dport 2375 -log nolog # Docker API\n";
        let adopted = super::super::native::parse(captured);

        assert!(adopted.unmodeled.is_empty());
        let rendered = super::super::render::render(&adopted.managed);

        assert_eq!(
            super::super::render::semantic(captured),
            super::super::render::semantic(&rendered)
        );
    }
}
