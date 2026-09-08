use super::{ApiObject, ObjectResponse, ObjectsResponse, RawResponse, capture_with_events};
use crate::{client::PveClient, model::GuestKind, utility::progress::EventSink};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub(crate) struct PveSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) collected_at: DateTime<Utc>,
    pub(crate) mode: &'static str,
    pub(crate) endpoint: String,
    pub(crate) requests: ClusterResponses,
    pub(crate) nodes: BTreeMap<String, Node>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) enumeration_failures: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClusterResponses {
    pub(crate) version: RawResponse,
    pub(crate) cluster_status: ObjectsResponse,
    pub(crate) cluster_resources: ObjectsResponse,
    pub(crate) backup_jobs: ObjectsResponse,
    pub(crate) ha_status: RawResponse,
    pub(crate) pools: ObjectsResponse,
    pub(crate) storage: ObjectsResponse,
    pub(crate) firewall_options: ObjectResponse,
    pub(crate) firewall_rules: ObjectsResponse,
    pub(crate) firewall_groups: ObjectsResponse,
    pub(crate) firewall_aliases: ObjectsResponse,
}

#[derive(Debug, Serialize)]
pub(crate) struct Node {
    pub(crate) status: ObjectResponse,
    pub(crate) network: ObjectsResponse,
    pub(crate) dns: ObjectResponse,
    pub(crate) storage: ObjectsResponse,
    pub(crate) firewall_options: ObjectResponse,
    pub(crate) firewall_rules: ObjectsResponse,
    pub(crate) lxcs: BTreeMap<String, Guest>,
    pub(crate) vms: BTreeMap<String, Guest>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Guest {
    pub(crate) summary: ApiObject,
    pub(crate) config: ObjectResponse,
    pub(crate) snapshots: ObjectsResponse,
    pub(crate) firewall_options: ObjectResponse,
    pub(crate) firewall_rules: ObjectsResponse,
}

pub(crate) fn capture_pve(client: &dyn PveClient, events: &dyn EventSink) -> PveSnapshot {
    let requests = ClusterResponses {
        version: get(client, "/version", events),
        cluster_status: get(client, "/cluster/status", events),
        cluster_resources: get(client, "/cluster/resources", events),
        backup_jobs: get(client, "/cluster/backup", events),
        ha_status: get(client, "/cluster/ha/status/current", events),
        pools: get(client, "/pools", events),
        storage: get(client, "/storage", events),
        firewall_options: get(client, "/cluster/firewall/options", events),
        firewall_rules: get(client, "/cluster/firewall/rules", events),
        firewall_groups: get(client, "/cluster/firewall/groups", events),
        firewall_aliases: get(client, "/cluster/firewall/aliases", events),
    };

    let mut enumeration_failures = Vec::new();
    let mut node_names = Vec::new();
    if let Some(items) = &requests.cluster_status.data {
        for (index, item) in items.iter().enumerate() {
            let Some(kind) = item.get("type").and_then(Value::as_str) else {
                enumeration_failures.push(format!(
                    "/cluster/status[{index}]: missing string field type"
                ));
                continue;
            };
            if kind != "node" {
                continue;
            }
            match item.get("name").and_then(Value::as_str) {
                Some(name) if !name.is_empty() => node_names.push(name.to_owned()),
                _ => enumeration_failures.push(format!(
                    "/cluster/status[{index}]: node has no non-empty string field name"
                )),
            }
        }
    }
    node_names.sort();
    let mut unique_nodes = BTreeSet::new();
    node_names.retain(|node| {
        if unique_nodes.insert(node.clone()) {
            true
        } else {
            enumeration_failures.push(format!("/cluster/status: duplicate node identity {node}"));
            false
        }
    });

    let mut nodes = BTreeMap::new();
    for node in node_names {
        let lxc_list = get(client, &format!("/nodes/{node}/lxc"), events);
        let qemu_list = get(client, &format!("/nodes/{node}/qemu"), events);
        let (lxcs, lxc_failures) = capture_guests(client, &node, GuestKind::Lxc, &lxc_list, events);
        let (vms, vm_failures) = capture_guests(client, &node, GuestKind::Qemu, &qemu_list, events);
        enumeration_failures.extend(lxc_failures);
        enumeration_failures.extend(vm_failures);
        nodes.insert(
            node.clone(),
            Node {
                status: get(client, &format!("/nodes/{node}/status"), events),
                network: get(client, &format!("/nodes/{node}/network"), events),
                dns: get(client, &format!("/nodes/{node}/dns"), events),
                storage: get(client, &format!("/nodes/{node}/storage"), events),
                firewall_options: get(client, &format!("/nodes/{node}/firewall/options"), events),
                firewall_rules: get(client, &format!("/nodes/{node}/firewall/rules"), events),
                lxcs,
                vms,
            },
        );
    }

    PveSnapshot {
        schema_version: 1,
        collected_at: Utc::now(),
        mode: "read-only",
        endpoint: client.endpoint().into(),
        requests,
        nodes,
        enumeration_failures,
    }
}

impl PveSnapshot {
    pub(crate) fn failures(&self) -> Vec<String> {
        let mut failures = self.enumeration_failures.clone();
        add_failure("cluster", &self.requests.version, &mut failures);
        add_failure("cluster", &self.requests.cluster_status, &mut failures);
        add_failure("cluster", &self.requests.cluster_resources, &mut failures);
        add_failure("cluster", &self.requests.backup_jobs, &mut failures);
        add_failure("cluster", &self.requests.ha_status, &mut failures);
        add_failure("cluster", &self.requests.pools, &mut failures);
        add_failure("cluster", &self.requests.storage, &mut failures);
        add_failure("cluster", &self.requests.firewall_options, &mut failures);
        add_failure("cluster", &self.requests.firewall_rules, &mut failures);
        add_failure("cluster", &self.requests.firewall_groups, &mut failures);
        add_failure("cluster", &self.requests.firewall_aliases, &mut failures);
        for (node_name, node) in &self.nodes {
            add_failure(node_name, &node.status, &mut failures);
            add_failure(node_name, &node.network, &mut failures);
            add_failure(node_name, &node.dns, &mut failures);
            add_failure(node_name, &node.storage, &mut failures);
            add_failure(node_name, &node.firewall_options, &mut failures);
            add_failure(node_name, &node.firewall_rules, &mut failures);
            for (kind, guests) in [(GuestKind::Lxc, &node.lxcs), (GuestKind::Qemu, &node.vms)] {
                for (vmid, guest) in guests {
                    let prefix = format!("{node_name}/{kind}/{vmid}");
                    add_failure(&prefix, &guest.config, &mut failures);
                    add_failure(&prefix, &guest.snapshots, &mut failures);
                    add_failure(&prefix, &guest.firewall_options, &mut failures);
                    add_failure(&prefix, &guest.firewall_rules, &mut failures);
                }
            }
        }
        failures
    }
}

fn capture_guests(
    client: &dyn PveClient,
    node: &str,
    kind: GuestKind,
    list: &ObjectsResponse,
    events: &dyn EventSink,
) -> (BTreeMap<String, Guest>, Vec<String>) {
    let mut guests = BTreeMap::new();
    let mut failures = Vec::new();
    let Some(items) = list.data.as_ref() else {
        return (guests, failures);
    };
    for (index, summary) in items.iter().enumerate() {
        let vmid = summary.get("vmid").and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
                .and_then(|value| u32::try_from(value).ok())
        });
        let Some(vmid) = vmid else {
            failures.push(format!(
                "/nodes/{node}/{kind}[{index}]: missing or invalid vmid"
            ));
            continue;
        };
        let base = format!("/nodes/{node}/{kind}/{vmid}");
        let guest = Guest {
            summary: summary.clone(),
            config: get(client, &format!("{base}/config"), events),
            snapshots: get(client, &format!("{base}/snapshot"), events),
            firewall_options: get(client, &format!("{base}/firewall/options"), events),
            firewall_rules: get(client, &format!("{base}/firewall/rules"), events),
        };
        if guests.insert(vmid.to_string(), guest).is_some() {
            failures.push(format!(
                "/nodes/{node}/{kind}: duplicate guest identity {vmid}"
            ));
        }
    }
    (guests, failures)
}

fn get<T: serde::de::DeserializeOwned>(
    client: &dyn PveClient,
    path: &str,
    events: &dyn EventSink,
) -> super::CapturedResponse<T> {
    capture_with_events(path, || client.get(path), events)
}

fn add_failure<T>(prefix: &str, response: &super::CapturedResponse<T>, failures: &mut Vec<String>) {
    if let Some(error) = response.failure() {
        failures.push(format!("{prefix}{}: {error}", response.path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    struct FakePve;

    impl PveClient for FakePve {
        fn endpoint(&self) -> &str {
            "https://pve.test:8006"
        }

        fn get(&self, path: &str) -> Result<Value> {
            if path.ends_with("/snapshot") || path.ends_with("/firewall/rules") {
                Ok(serde_json::json!([]))
            } else {
                Ok(serde_json::json!({}))
            }
        }

        fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }

        fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }

        fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }
    }

    #[test]
    fn guest_enumeration_accepts_numeric_strings() {
        let list = crate::discovery::response::capture("/nodes/pve/qemu", || {
            Ok(serde_json::json!([{ "vmid": "101" }]))
        });

        let (guests, failures) = capture_guests(
            &FakePve,
            "pve",
            GuestKind::Qemu,
            &list,
            &crate::utility::progress::NullEventSink,
        );

        assert!(guests.contains_key("101"));
        assert!(failures.is_empty());
    }

    #[test]
    fn malformed_and_duplicate_guest_identities_are_failures() {
        let list = crate::discovery::response::capture("/nodes/pve/lxc", || {
            Ok(serde_json::json!([
                { "vmid": "not-a-number" },
                { "vmid": 101 },
                { "vmid": "101" }
            ]))
        });

        let (_, failures) = capture_guests(
            &FakePve,
            "pve",
            GuestKind::Lxc,
            &list,
            &crate::utility::progress::NullEventSink,
        );

        assert_eq!(failures.len(), 2);
        assert!(failures[0].contains("missing or invalid vmid"));
        assert!(failures[1].contains("duplicate guest identity 101"));
    }
}
