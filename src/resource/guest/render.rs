use crate::model::{BindMount, Nic, VmNic};

pub fn options(items: Vec<(&str, Option<String>)>) -> String {
    items
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
        .collect::<Vec<_>>()
        .join(",")
}
pub fn lxc_nic(n: &Nic) -> String {
    options(vec![
        ("name", Some(n.name.clone())),
        ("bridge", Some(n.bridge.clone())),
        ("firewall", Some(u8::from(n.firewall).to_string())),
        ("gw", n.gateway4.clone()),
        ("gw6", n.gateway6.clone()),
        ("hwaddr", Some(n.mac.clone())),
        ("ip", Some(n.ipv4.clone())),
        ("ip6", n.ipv6.clone()),
        ("type", Some("veth".into())),
    ])
}
pub fn vm_nic(n: &VmNic) -> String {
    options(vec![
        (n.model.as_str(), Some(n.mac.clone())),
        ("bridge", Some(n.bridge.clone())),
        ("firewall", Some(u8::from(n.firewall).to_string())),
        ("tag", n.vlan.map(|value| value.to_string())),
    ])
}
pub fn bind_mount(m: &BindMount) -> String {
    format!(
        "{},mp={},backup={}",
        m.source,
        m.target,
        u8::from(m.backed_up_by_pve)
    )
}
