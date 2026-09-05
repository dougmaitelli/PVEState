use crate::model::Network;

pub(crate) fn render(n: &Network) -> String {
    let mut s = String::from("# Managed by PVE State\nauto lo\niface lo inet loopback\n\n");
    for i in &n.interfaces {
        s += &format!("iface {} inet {}\n\n", i.name, i.method);
    }
    for b in &n.bridges {
        s += &format!("auto {}\niface {} inet {}\n", b.name, b.name, b.method);
        if let Some(x) = &b.address {
            s += &format!("    address {x}\n");
        }
        if let Some(x) = &b.gateway {
            s += &format!("    gateway {x}\n");
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
pub(crate) fn semantic_lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|x| !x.is_empty() && !x.starts_with('#'))
        .map(str::to_string)
        .collect()
}
