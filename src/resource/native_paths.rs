pub(crate) const NETWORK_ARTIFACT: &str = "network/interfaces";
pub(crate) const NETWORK_REMOTE: &str = "/etc/network/interfaces";
pub(crate) const CLUSTER_FIREWALL_ARTIFACT: &str = "pve/firewall/cluster.fw";
pub(crate) const CLUSTER_FIREWALL_REMOTE: &str = "/etc/pve/firewall/cluster.fw";
pub(crate) const FIREWALL_ARTIFACT_ROOT: &str = "pve/firewall";

pub(crate) fn node_firewall_artifact(node: &str) -> String {
    format!("{FIREWALL_ARTIFACT_ROOT}/{node}-host.fw")
}

pub(crate) fn node_firewall_remote(node: &str) -> String {
    format!("/etc/pve/nodes/{node}/host.fw")
}

pub(crate) fn guest_firewall_artifact(id: impl std::fmt::Display) -> String {
    format!("{FIREWALL_ARTIFACT_ROOT}/{id}.fw")
}

pub(crate) fn guest_firewall_remote(id: impl std::fmt::Display) -> String {
    format!("/etc/pve/firewall/{id}.fw")
}
