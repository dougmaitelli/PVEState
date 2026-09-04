//! Compatibility facade for renderers owned by resource domains.

pub use crate::resource::guest::render::{bind_mount, lxc_nic, options, vm_nic};
pub use crate::resource::network::render::semantic_lines;

pub fn network(network: &crate::model::Network) -> String {
    crate::resource::network::render::render(network)
}

pub fn firewall_policy(policy: &crate::model::FirewallPolicy) -> String {
    crate::resource::firewall::render::render(policy)
}

pub fn firewall_semantic(content: &str) -> (Vec<String>, Vec<String>) {
    crate::resource::firewall::render::semantic(content)
}
