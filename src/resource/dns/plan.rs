use crate::{
    client::PveClient,
    config::LocalState,
    reconcile::{ApiMethod, ApiTarget, Domain, Operation, PlanBuilder, ResourceId, before_values},
};
use anyhow::Result;
use std::collections::BTreeMap;

pub(crate) fn plan(
    local: &LocalState,
    pve: &dyn PveClient,
    builder: &mut PlanBuilder,
) -> Result<()> {
    let endpoint = format!("/nodes/{}/dns", local.guests.node);
    let actual = pve.get(&endpoint)?;
    let changes = changes(
        &local.network.dns.search,
        &local.network.dns.servers,
        &actual,
    );
    if !changes.is_empty() {
        builder.operations().push(Operation::ApiMutation {
            target: ApiTarget::Pve,
            method: ApiMethod::Put,
            domain: Domain::Dns,
            resource: ResourceId::Named(local.guests.node.clone()),
            endpoint: endpoint.into(),
            before_values: before_values(&changes, &actual),
            changes,
            environment_changes: BTreeMap::new(),
            digest: None,
        });
    }
    Ok(())
}

fn changes(
    search: &str,
    servers: &[String],
    actual: &serde_json::Value,
) -> BTreeMap<String, String> {
    let mut changes = BTreeMap::new();
    compare(&mut changes, "search", search, actual);
    for (index, server) in servers.iter().enumerate() {
        compare(&mut changes, &format!("dns{}", index + 1), server, actual);
    }
    let delete = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| key.starts_with("dns"))
        .filter_map(|key| key[3..].parse::<usize>().ok().map(|index| (key, index)))
        .filter(|(_, index)| *index > servers.len())
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if !delete.is_empty() {
        changes.insert("delete".into(), delete.join(","));
    }
    changes
}

fn compare(
    changes: &mut BTreeMap<String, String>,
    key: &str,
    wanted: &str,
    actual: &serde_json::Value,
) {
    let current = actual.get(key).map(value_string);
    if current.as_deref() != Some(wanted) {
        changes.insert(key.into(), wanted.into());
    }
}

fn value_string(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_servers_are_explicitly_deleted() {
        let actual =
            serde_json::json!({"search":"example.test", "dns1":"1.1.1.1", "dns2":"8.8.8.8"});
        let changes = changes("example.test", &["1.1.1.1".into()], &actual);

        assert_eq!(changes.get("delete").map(String::as_str), Some("dns2"));
    }
}
