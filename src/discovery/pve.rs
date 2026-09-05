use super::{ApiObject, ObjectResponse, ObjectsResponse, RawResponse, capture_with_events};
use crate::{client::PveClient, model::GuestKind, utility::progress::EventSink};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub(crate) struct PveSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) collected_at: DateTime<Utc>,
    pub(crate) mode: &'static str,
    pub(crate) endpoint: String,
    pub(crate) requests: ClusterResponses,
    pub(crate) nodes: BTreeMap<String, Node>,
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

    let mut node_names: Vec<String> = requests
        .cluster_status
        .data
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("node"))
        .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect();
    node_names.sort();

    let mut nodes = BTreeMap::new();
    for node in node_names {
        let lxc_list = get(client, &format!("/nodes/{node}/lxc"), events);
        let qemu_list = get(client, &format!("/nodes/{node}/qemu"), events);
        nodes.insert(
            node.clone(),
            Node {
                status: get(client, &format!("/nodes/{node}/status"), events),
                network: get(client, &format!("/nodes/{node}/network"), events),
                dns: get(client, &format!("/nodes/{node}/dns"), events),
                storage: get(client, &format!("/nodes/{node}/storage"), events),
                firewall_options: get(client, &format!("/nodes/{node}/firewall/options"), events),
                firewall_rules: get(client, &format!("/nodes/{node}/firewall/rules"), events),
                lxcs: capture_guests(client, &node, GuestKind::Lxc, &lxc_list, events),
                vms: capture_guests(client, &node, GuestKind::Qemu, &qemu_list, events),
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
    }
}

impl PveSnapshot {
    pub(crate) fn failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
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
) -> BTreeMap<String, Guest> {
    let mut guests = BTreeMap::new();
    let Some(items) = list.data.as_ref() else {
        return guests;
    };
    for summary in items {
        let Some(vmid) = summary.get("vmid").and_then(Value::as_u64) else {
            continue;
        };
        let base = format!("/nodes/{node}/{kind}/{vmid}");
        guests.insert(
            vmid.to_string(),
            Guest {
                summary: summary.clone(),
                config: get(client, &format!("{base}/config"), events),
                snapshots: get(client, &format!("{base}/snapshot"), events),
                firewall_options: get(client, &format!("{base}/firewall/options"), events),
                firewall_rules: get(client, &format!("{base}/firewall/rules"), events),
            },
        );
    }
    guests
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
