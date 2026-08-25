use crate::model::{FirewallPolicy, Network, Nic, VmNic};

pub fn options(items: Vec<(&str, Option<String>)>) -> String {
    items
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}={v}")))
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
        ("tag", n.vlan.map(|x| x.to_string())),
    ])
}

pub fn network(n: &Network) -> String {
    let mut s = String::from("# Managed by PVE State\nauto lo\niface lo inet loopback\n\n");
    for i in &n.interfaces {
        s += &format!("iface {} inet {}\n\n", i.name, i.method)
    }
    for b in &n.bridges {
        s += &format!("auto {}\niface {} inet {}\n", b.name, b.name, b.method);
        if let Some(x) = &b.address {
            s += &format!("    address {x}\n")
        }
        if let Some(x) = &b.gateway {
            s += &format!("    gateway {x}\n")
        }
        s += &format!(
            "    bridge-ports {}\n    bridge-stp {}\n    bridge-fd {}\n\n",
            if b.ports.is_empty() {
                "none".into()
            } else {
                b.ports.join(" ")
            },
            if b.stp { "on" } else { "off" },
            b.forward_delay
        )
    }
    s += "source /etc/network/interfaces.d/*\n";
    s
}

pub fn firewall_policy(policy: &FirewallPolicy) -> String {
    let mut s = String::from("[OPTIONS]\n\n");
    s += &format!("enable: {}\n", u8::from(policy.enabled));
    if let Some(x) = &policy.log_level_in {
        s += &format!("log_level_in: {x}\n")
    }
    if !policy.rules.is_empty() {
        s += "\n[RULES]\n\n";
        for rule in &policy.rules {
            if !rule.enabled {
                s.push('|')
            }
            s += &format!("{} {}", rule.direction, rule.action);
            if let Some(v) = &rule.interface {
                s += &format!(" -i {v}")
            }
            if let Some(v) = &rule.protocol {
                s += &format!(" -p {v}")
            }
            if let Some(v) = &rule.destination_port {
                s += &format!(" -dport {v}")
            }
            s += &format!(" -log {}", rule.log);
            if let Some(v) = &rule.comment {
                s += &format!(" # {v}")
            }
            s.push('\n')
        }
    }
    s
}

pub fn semantic_lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|x| !x.is_empty() && !x.starts_with('#'))
        .map(str::to_string)
        .collect()
}

pub fn firewall_semantic(s: &str) -> (Vec<String>, Vec<String>) {
    let mut section = "";
    let mut options = Vec::new();
    let mut rules = Vec::new();
    for line in semantic_lines(s) {
        if line.starts_with('[') {
            section = if line == "[OPTIONS]" {
                "options"
            } else {
                "rules"
            };
        } else if section == "options" {
            options.push(line);
        } else if section == "rules" {
            rules.push(line);
        }
    }
    options.sort();
    (options, rules)
}
