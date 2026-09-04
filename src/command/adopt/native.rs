use super::yaml::{self, Patch, Segment};
use crate::{
    config::Repository,
    model::{Bridge, FirewallPolicy, FirewallRule, Interface, Network},
    utility::atomic_file,
};
use anyhow::{Context, Result, bail};
use std::{fs, path::Path};

#[derive(Debug, Clone)]
pub enum Target {
    Network,
    Firewall { resource: String },
}

pub fn adopt(repo: &Repository, target: &Target) -> Result<&'static str> {
    match target {
        Target::Network => {
            let captured = fs::read_to_string(repo.observed().join("network/interfaces"))?;
            let parsed = parse_network(&captured, &repo.network)?;
            patch_document(
                &repo.root.join("config/network.yml"),
                vec![
                    set("interfaces", &parsed.interfaces)?,
                    set("bridges", &parsed.bridges)?,
                ],
            )?;
            Ok("config/network.yml")
        },
        Target::Firewall { resource } => {
            let (observed, config, path) = firewall_paths(repo, resource)?;
            let policy = fs::read_to_string(observed)
                .ok()
                .map(|content| parse_firewall(&content))
                .transpose()?;
            patch_document(
                &config,
                vec![Patch::Set(path, serde_yaml::to_value(policy)?)],
            )?;
            Ok(match resource.as_str() {
                "cluster" => "config/cluster.yml",
                value if value.starts_with("node/") => "config/node.yml",
                _ => "config/guests.yml",
            })
        },
    }
}

fn set(key: &str, value: &impl serde::Serialize) -> Result<Patch> {
    Ok(Patch::Set(
        vec![Segment::Key(key.into())],
        serde_yaml::to_value(value)?,
    ))
}

fn patch_document(path: &Path, patches: Vec<Patch>) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let updated = yaml::apply_patches(&content, &patches)?;
    atomic_file::write(path, updated.as_bytes())
}

fn firewall_paths(
    repo: &Repository,
    resource: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf, Vec<Segment>)> {
    let observed = repo.observed().join("pve/firewall");
    let config = repo.root.join("config");
    if resource == "cluster" {
        return Ok((
            observed.join("cluster.fw"),
            config.join("cluster.yml"),
            vec![Segment::Key("firewall".into())],
        ));
    }
    if let Some(node) = resource.strip_prefix("node/") {
        return Ok((
            observed.join(format!("{node}-host.fw")),
            config.join("node.yml"),
            vec![Segment::Key("firewall".into())],
        ));
    }
    let id: u32 = resource
        .parse()
        .with_context(|| format!("invalid firewall resource {resource}"))?;
    let (collection, exists) = if repo.guests.lxcs.contains_key(&id) {
        ("lxcs", true)
    } else {
        ("vms", repo.guests.vms.contains_key(&id))
    };
    if !exists {
        bail!("firewall resource {resource} is not a configured guest")
    }
    Ok((
        observed.join(format!("{id}.fw")),
        config.join("guests.yml"),
        vec![
            Segment::Key(collection.into()),
            Segment::Key(id.to_string()),
            Segment::Key("firewall".into()),
        ],
    ))
}

fn parse_firewall(content: &str) -> Result<FirewallPolicy> {
    let mut section = "";
    let mut enabled = false;
    let mut log_level_in = None;
    let mut rules = Vec::new();

    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section == "[OPTIONS]" {
            if let Some((key, value)) = line.split_once(':') {
                match key.trim() {
                    "enable" => enabled = matches!(value.trim(), "1" | "yes" | "true" | "on"),
                    "log_level_in" => log_level_in = Some(value.trim().into()),
                    _ => {},
                }
            }
        } else if section == "[RULES]" {
            rules.push(parse_firewall_rule(line)?);
        }
    }

    Ok(FirewallPolicy {
        enabled,
        log_level_in,
        rules,
    })
}

fn parse_firewall_rule(line: &str) -> Result<FirewallRule> {
    let (enabled, line) = line
        .strip_prefix('|')
        .map_or((true, line), |line| (false, line));
    let (rule, comment) = line
        .split_once('#')
        .map_or((line, None), |(rule, comment)| {
            (rule, Some(comment.trim().to_string()))
        });
    let mut words = rule.split_whitespace();
    let direction = words.next().context("firewall rule direction")?.into();
    let action = words.next().context("firewall rule action")?.into();
    let mut interface = None;
    let mut protocol = None;
    let mut destination_port = None;
    let mut log = "nolog".to_string();
    while let Some(flag) = words.next() {
        let value = words
            .next()
            .with_context(|| format!("firewall rule value after {flag}"))?;
        match flag {
            "-i" => interface = Some(value.into()),
            "-p" => protocol = Some(value.into()),
            "-dport" => destination_port = Some(value.into()),
            "-log" => log = value.into(),
            _ => bail!("unsupported captured firewall rule option {flag}"),
        }
    }
    Ok(FirewallRule {
        enabled,
        direction,
        action,
        interface,
        protocol,
        destination_port,
        log,
        comment,
    })
}

fn parse_network(content: &str, current: &Network) -> Result<Network> {
    #[derive(Default)]
    struct Stanza {
        name: String,
        family: String,
        method: String,
        address: Option<String>,
        gateway: Option<String>,
        ports: Option<Vec<String>>,
        stp: Option<bool>,
        forward_delay: Option<u16>,
    }

    fn finish(stanza: Option<Stanza>, interfaces: &mut Vec<Interface>, bridges: &mut Vec<Bridge>) {
        let Some(stanza) = stanza else { return };
        if stanza.name == "lo" {
            return;
        }
        if stanza.family == "inet6" {
            if let Some(bridge) = bridges.iter_mut().find(|bridge| bridge.name == stanza.name) {
                bridge.ipv6 = stanza.address;
                bridge.gateway6 = stanza.gateway;
            }
        } else if let Some(ports) = stanza.ports {
            bridges.push(Bridge {
                name: stanza.name,
                method: stanza.method,
                address: stanza.address,
                gateway: stanza.gateway,
                ipv6: None,
                gateway6: None,
                ports,
                stp: stanza.stp.unwrap_or(false),
                forward_delay: stanza.forward_delay.unwrap_or_default(),
            });
        } else {
            interfaces.push(Interface {
                name: stanza.name,
                method: stanza.method,
            });
        }
    }

    let mut interfaces = Vec::new();
    let mut bridges = Vec::new();
    let mut stanza: Option<Stanza> = None;
    for line in content.lines().map(str::trim) {
        if let Some(definition) = line.strip_prefix("iface ") {
            finish(stanza.take(), &mut interfaces, &mut bridges);
            let parts = definition.split_whitespace().collect::<Vec<_>>();
            if parts.len() >= 3 {
                stanza = Some(Stanza {
                    name: parts[0].into(),
                    family: parts[1].into(),
                    method: parts[2].into(),
                    ..Stanza::default()
                });
            }
            continue;
        }
        let Some(active) = stanza.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        match key {
            "address" => active.address = Some(value.into()),
            "gateway" => active.gateway = Some(value.into()),
            "bridge-ports" => {
                active.ports = Some(if value == "none" {
                    Vec::new()
                } else {
                    value.split_whitespace().map(str::to_string).collect()
                })
            },
            "bridge-stp" => active.stp = Some(matches!(value, "on" | "yes" | "1")),
            "bridge-fd" => active.forward_delay = value.parse().ok(),
            _ => {},
        }
    }
    finish(stanza, &mut interfaces, &mut bridges);

    let mut adopted = current.clone();
    adopted.interfaces = interfaces;
    adopted.bridges = bridges;
    Ok(adopted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_disabled_firewall_rules() {
        let policy = parse_firewall(
            "[OPTIONS]\nenable: 1\n\n[RULES]\n|IN ACCEPT -p tcp -dport 443 -log nolog # Web\n",
        )
        .unwrap();
        assert!(policy.enabled);
        assert!(!policy.rules[0].enabled);
        assert_eq!(policy.rules[0].destination_port.as_deref(), Some("443"));
        assert_eq!(policy.rules[0].comment.as_deref(), Some("Web"));
    }

    #[test]
    fn parses_ipv4_and_ipv6_bridge_configuration() {
        let current: Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let captured = "iface eno1 inet manual\n\nauto vmbr0\niface vmbr0 inet static\n\taddress 192.0.2.10/24\n\tgateway 192.0.2.1\n\tbridge-ports eno1\n\tbridge-stp off\n\tbridge-fd 0\n\niface vmbr0 inet6 static\n\taddress 2001:db8::10/64\n\tgateway 2001:db8::1\n";

        let parsed = parse_network(captured, &current).unwrap();

        assert_eq!(parsed.interfaces[0].name, "eno1");
        assert_eq!(parsed.bridges[0].name, "vmbr0");
        assert_eq!(parsed.bridges[0].ipv6.as_deref(), Some("2001:db8::10/64"));
        assert_eq!(parsed.bridges[0].gateway6.as_deref(), Some("2001:db8::1"));
    }
}
