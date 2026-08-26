use crate::{client::PveClient, model::GuestKind};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const CLUSTER_ENDPOINTS: [(&str, &str); 11] = [
    ("version", "/version"),
    ("cluster_status", "/cluster/status"),
    ("cluster_resources", "/cluster/resources"),
    ("backup_jobs", "/cluster/backup"),
    ("ha_status", "/cluster/ha/status/current"),
    ("pools", "/pools"),
    ("storage", "/storage"),
    ("firewall_options", "/cluster/firewall/options"),
    ("firewall_rules", "/cluster/firewall/rules"),
    ("firewall_groups", "/cluster/firewall/groups"),
    ("firewall_aliases", "/cluster/firewall/aliases"),
];

#[derive(Debug, Serialize)]
pub struct PveSnapshot {
    pub schema_version: u8,
    pub collected_at: DateTime<Utc>,
    pub mode: &'static str,
    pub endpoint: String,
    pub requests: BTreeMap<String, Response>,
    pub nodes: BTreeMap<String, Node>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub status: Response,
    pub network: Response,
    pub dns: Response,
    pub storage: Response,
    pub firewall_options: Response,
    pub firewall_rules: Response,
    pub lxcs: BTreeMap<String, Guest>,
    pub vms: BTreeMap<String, Guest>,
}

#[derive(Debug, Serialize)]
pub struct Guest {
    pub summary: Value,
    pub config: Response,
    pub snapshots: Response,
    pub firewall_options: Response,
    pub firewall_rules: Response,
}

pub fn capture_pve(client: &dyn PveClient) -> PveSnapshot {
    let mut requests = BTreeMap::new();
    for (name, path) in CLUSTER_ENDPOINTS {
        requests.insert(name.into(), safe_get(client, path));
    }

    let mut node_names: Vec<String> = requests
        .get("cluster_status")
        .and_then(|response| response.data.as_ref())
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("node"))
        .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect();
    node_names.sort();

    let mut nodes = BTreeMap::new();
    for node in node_names {
        let lxc_list = safe_get(client, &format!("/nodes/{node}/lxc"));
        let qemu_list = safe_get(client, &format!("/nodes/{node}/qemu"));
        nodes.insert(
            node.clone(),
            Node {
                status: safe_get(client, &format!("/nodes/{node}/status")),
                network: safe_get(client, &format!("/nodes/{node}/network")),
                dns: safe_get(client, &format!("/nodes/{node}/dns")),
                storage: safe_get(client, &format!("/nodes/{node}/storage")),
                firewall_options: safe_get(client, &format!("/nodes/{node}/firewall/options")),
                firewall_rules: safe_get(client, &format!("/nodes/{node}/firewall/rules")),
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
        collect_failures("cluster", self.requests.values(), &mut failures);
        for (node_name, node) in &self.nodes {
            collect_failures(
                node_name,
                [
                    &node.status,
                    &node.network,
                    &node.dns,
                    &node.storage,
                    &node.firewall_options,
                    &node.firewall_rules,
                ],
                &mut failures,
            );
            for (kind, guests) in [(GuestKind::Lxc, &node.lxcs), (GuestKind::Qemu, &node.vms)] {
                for (vmid, guest) in guests {
                    collect_failures(
                        &format!("{node_name}/{kind}/{vmid}"),
                        [
                            &guest.config,
                            &guest.snapshots,
                            &guest.firewall_options,
                            &guest.firewall_rules,
                        ],
                        &mut failures,
                    );
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
    list: &Response,
) -> BTreeMap<String, Guest> {
    let mut guests = BTreeMap::new();
    let Some(items) = list.data.as_ref().and_then(Value::as_array) else {
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
                config: safe_get(client, &format!("{base}/config")),
                snapshots: safe_get(client, &format!("{base}/snapshot")),
                firewall_options: safe_get(client, &format!("{base}/firewall/options")),
                firewall_rules: safe_get(client, &format!("{base}/firewall/rules")),
            },
        );
    }
    guests
}

fn safe_get(client: &dyn PveClient, path: &str) -> Response {
    match client.get(path) {
        Ok(data) => Response {
            ok: true,
            path: path.into(),
            data: Some(data),
            error: None,
        },
        Err(error) => Response {
            ok: false,
            path: path.into(),
            data: None,
            error: Some(format!("{error:#}")),
        },
    }
}

fn collect_failures<'a>(
    prefix: &str,
    responses: impl IntoIterator<Item = &'a Response>,
    failures: &mut Vec<String>,
) {
    for response in responses {
        if let Some(error) = &response.error {
            failures.push(format!("{prefix}{}: {error}", response.path));
        }
    }
}
