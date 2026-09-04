use crate::model::{BindMount, FirewallPolicy, FirewallRule, Network, Nic, VmNic};

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

pub fn bind_mount(mount: &BindMount) -> String {
    format!(
        "{},mp={},backup={}",
        mount.source,
        mount.target,
        u8::from(mount.backed_up_by_pve)
    )
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
        );
        if let Some(address) = &b.ipv6 {
            s += &format!("iface {} inet6 static\n    address {address}\n", b.name);
            if let Some(gateway) = &b.gateway6 {
                s += &format!("    gateway {gateway}\n");
            }
            s.push('\n');
        }
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
    for (key, value) in &policy.options {
        s += &format!("{key}: {value}\n");
    }
    if !policy.aliases.is_empty() {
        s += "\n[ALIASES]\n\n";
        for alias in &policy.aliases {
            s += &format!("{} {}", alias.name, alias.network);
            append_comment(&mut s, alias.comment.as_deref());
        }
    }
    for ip_set in &policy.ip_sets {
        s += &format!("\n[IPSET {}]\n\n", ip_set.name);
        for entry in &ip_set.entries {
            s += &entry.network;
            if entry.nomatch {
                s += " nomatch";
            }
            append_comment(&mut s, entry.comment.as_deref());
        }
    }
    for group in &policy.security_groups {
        s += &format!("\n[group {}]\n\n", group.name);
        render_firewall_rules(&mut s, &group.rules);
    }
    if !policy.rules.is_empty() {
        s += "\n[RULES]\n\n";
        render_firewall_rules(&mut s, &policy.rules);
    }
    s
}

fn render_firewall_rules(output: &mut String, rules: &[FirewallRule]) {
    for rule in rules {
        if !rule.enabled {
            output.push('|')
        }
        *output += &format!("{} ", rule.direction);
        if let Some(name) = &rule.macro_name {
            *output += &format!("{name}({})", rule.action);
        } else {
            *output += &rule.action;
        }
        if let Some(value) = &rule.interface {
            *output += &format!(" -i {value}")
        }
        if let Some(value) = &rule.protocol {
            *output += &format!(" -p {value}")
        }
        if let Some(value) = &rule.source {
            *output += &format!(" -source {value}")
        }
        if let Some(value) = &rule.destination {
            *output += &format!(" -dest {value}")
        }
        if let Some(value) = &rule.source_port {
            *output += &format!(" -sport {value}")
        }
        if let Some(value) = &rule.destination_port {
            *output += &format!(" -dport {value}")
        }
        *output += &format!(" -log {}", rule.log);
        append_comment(output, rule.comment.as_deref());
    }
}

fn append_comment(output: &mut String, comment: Option<&str>) {
    if let Some(value) = comment {
        *output += &format!(" # {value}");
    }
    output.push('\n');
}

pub fn semantic_lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|x| !x.is_empty() && !x.starts_with('#'))
        .map(str::to_string)
        .collect()
}

pub fn firewall_semantic(s: &str) -> (Vec<String>, Vec<String>) {
    let mut section = String::new();
    let mut options = Vec::new();
    let mut rules = Vec::new();
    for line in semantic_lines(s) {
        if line.starts_with('[') {
            section.clone_from(&line);
            if line == "[RULES]" || line.starts_with("[group ") {
                rules.push(line);
            }
        } else if section == "[OPTIONS]" || section == "[ALIASES]" || section.starts_with("[IPSET ")
        {
            options.push(format!("{section}:{line}"));
        } else if section == "[RULES]" || section.starts_with("[group ") {
            rules.push(line);
        }
    }
    options.sort();
    (options, rules)
}
