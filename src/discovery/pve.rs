use super::{ApiObject, ObjectResponse, ObjectsResponse, RawResponse, capture as capture_response};
use crate::{client::PveClient, model::GuestKind};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct PveSnapshot {
    pub schema_version: u8,
    pub collected_at: DateTime<Utc>,
    pub mode: &'static str,
    pub endpoint: String,
    pub requests: ClusterResponses,
    pub nodes: BTreeMap<String, Node>,
}

#[derive(Debug, Serialize)]
pub struct ClusterResponses {
    pub version: RawResponse,
    pub cluster_status: ObjectsResponse,
    pub cluster_resources: ObjectsResponse,
    pub backup_jobs: ObjectsResponse,
    pub ha_status: RawResponse,
    pub pools: ObjectsResponse,
    pub storage: ObjectsResponse,
    pub firewall_options: ObjectResponse,
    pub firewall_rules: ObjectsResponse,
    pub firewall_groups: ObjectsResponse,
    pub firewall_aliases: ObjectsResponse,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub status: ObjectResponse,
    pub network: ObjectsResponse,
    pub dns: ObjectResponse,
    pub storage: ObjectsResponse,
    pub firewall_options: ObjectResponse,
    pub firewall_rules: ObjectsResponse,
    pub lxcs: BTreeMap<String, Guest>,
    pub vms: BTreeMap<String, Guest>,
}

#[derive(Debug, Serialize)]
pub struct Guest {
    pub summary: ApiObject,
    pub config: ObjectResponse,
    pub snapshots: ObjectsResponse,
    pub firewall_options: ObjectResponse,
    pub firewall_rules: ObjectsResponse,
}

pub fn capture_pve(client: &dyn PveClient) -> PveSnapshot {
    let requests = ClusterResponses {
        version: get(client, "/version"),
        cluster_status: get(client, "/cluster/status"),
        cluster_resources: get(client, "/cluster/resources"),
        backup_jobs: get(client, "/cluster/backup"),
        ha_status: get(client, "/cluster/ha/status/current"),
        pools: get(client, "/pools"),
        storage: get(client, "/storage"),
        firewall_options: get(client, "/cluster/firewall/options"),
        firewall_rules: get(client, "/cluster/firewall/rules"),
        firewall_groups: get(client, "/cluster/firewall/groups"),
        firewall_aliases: get(client, "/cluster/firewall/aliases"),
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
        let lxc_list = get(client, &format!("/nodes/{node}/lxc"));
        let qemu_list = get(client, &format!("/nodes/{node}/qemu"));
        nodes.insert(
            node.clone(),
            Node {
                status: get(client, &format!("/nodes/{node}/status")),
                network: get(client, &format!("/nodes/{node}/network")),
                dns: get(client, &format!("/nodes/{node}/dns")),
                storage: get(client, &format!("/nodes/{node}/storage")),
                firewall_options: get(client, &format!("/nodes/{node}/firewall/options")),
                firewall_rules: get(client, &format!("/nodes/{node}/firewall/rules")),
                lxcs: capture_guests(client, &node, GuestKind::Lxc, &lxc_list),
                vms: capture_guests(client, &node, GuestKind::Qemu, &qemu_list),
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
    pub fn failures(&self) -> Vec<String> {
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
                config: get(client, &format!("{base}/config")),
                snapshots: get(client, &format!("{base}/snapshot")),
                firewall_options: get(client, &format!("{base}/firewall/options")),
                firewall_rules: get(client, &format!("{base}/firewall/rules")),
            },
        );
    }
    guests
}

fn get<T: serde::de::DeserializeOwned>(
    client: &dyn PveClient,
    path: &str,
) -> super::CapturedResponse<T> {
    capture_response(path, || client.get(path))
}

fn add_failure<T>(prefix: &str, response: &super::CapturedResponse<T>, failures: &mut Vec<String>) {
    if let Some(error) = response.failure() {
        failures.push(format!("{prefix}{}: {error}", response.path));
    }
}
