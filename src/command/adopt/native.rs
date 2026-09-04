use super::yaml::{self, Patch, Segment};
use crate::{
    config::Repository,
    model::{Bridge, FirewallPolicy, FirewallRule, Interface, Network},
    utility::atomic_file,
};
use anyhow::{Context, Result, bail};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Debug, Clone)]
pub(crate) struct ParsedNetwork {
    pub managed: Network,
    pub unmodeled: Vec<NativeDirective>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeDirective {
    pub line: usize,
    pub text: String,
    pub reason: String,
}

impl NativeDirective {
    fn new(line: usize, text: &str, reason: impl Into<String>) -> Self {
        Self {
            line,
            text: text.to_string(),
            reason: reason.into(),
        }
    }

    pub(crate) fn diagnostic(&self) -> String {
        format!("line {} (`{}`): {}", self.line, self.text, self.reason)
    }
}

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
            if !parsed.unmodeled.is_empty() {
                bail!(
                    "captured network contains syntax that cannot be adopted safely: {}",
                    parsed
                        .unmodeled
                        .iter()
                        .map(NativeDirective::diagnostic)
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            }
            patch_document(
                &repo.root.join("config/network.yml"),
                vec![
                    set("interfaces", &parsed.managed.interfaces)?,
                    set("bridges", &parsed.managed.bridges)?,
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

pub(crate) fn parse_network(content: &str, current: &Network) -> Result<ParsedNetwork> {
    #[derive(Default)]
    struct Stanza {
        line: usize,
        definition: String,
        name: String,
        family: String,
        method: String,
        address: Option<String>,
        gateway: Option<String>,
        ports: Option<Vec<String>>,
        stp: Option<bool>,
        forward_delay: Option<u16>,
    }

    fn generated_comment(line: &str) -> bool {
        matches!(
            line,
            "#" | "# network interface settings; autogenerated"
                | "# Please do NOT modify this file directly, unless you know what"
                | "# you're doing."
                | "# If you want to manage parts of the network configuration manually,"
                | "# please utilize the 'source' or 'source-directory' directives to do"
                | "# so."
                | "# PVE will preserve these directives, but will NOT read its network"
                | "# configuration from sourced files, so do not attempt to move any of"
                | "# the PVE managed interfaces into external files!"
                | "# Managed by PVE State"
        )
    }

    let mut interfaces = Vec::new();
    let mut bridges = Vec::new();
    let mut stanzas = Vec::new();
    let mut automatic = Vec::new();
    let mut unmodeled = Vec::new();
    let mut stanza: Option<Stanza> = None;
    for (index, raw) in content.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.trim();
        if line.is_empty() || generated_comment(line) {
            continue;
        }
        if line.starts_with('#') {
            unmodeled.push(NativeDirective::new(
                line_number,
                line,
                "custom comments are not represented by the local network model",
            ));
            continue;
        }
        if let Some(definition) = line.strip_prefix("iface ") {
            if let Some(previous) = stanza.take() {
                stanzas.push(previous);
            }
            let parts = definition.split_whitespace().collect::<Vec<_>>();
            if parts.len() == 3 {
                stanza = Some(Stanza {
                    line: line_number,
                    definition: line.to_string(),
                    name: parts[0].into(),
                    family: parts[1].into(),
                    method: parts[2].into(),
                    ..Stanza::default()
                });
            } else {
                unmodeled.push(NativeDirective::new(
                    line_number,
                    line,
                    "malformed interface definition",
                ));
            }
            continue;
        }
        if let Some(names) = line.strip_prefix("auto ") {
            if let Some(previous) = stanza.take() {
                stanzas.push(previous);
            }
            automatic.extend(
                names
                    .split_whitespace()
                    .map(|name| (line_number, name.to_string())),
            );
            continue;
        }
        if line == "source /etc/network/interfaces.d/*" {
            if let Some(previous) = stanza.take() {
                stanzas.push(previous);
            }
            continue;
        }
        let Some(active) = stanza.as_mut() else {
            unmodeled.push(NativeDirective::new(
                line_number,
                line,
                "unsupported top-level network directive",
            ));
            continue;
        };
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            unmodeled.push(NativeDirective::new(
                line_number,
                line,
                "malformed interface directive",
            ));
            continue;
        };
        let value = value.trim();
        match key {
            "address" if active.address.is_none() => active.address = Some(value.into()),
            "address" => unmodeled.push(NativeDirective::new(
                line_number,
                line,
                "multiple addresses in one stanza are not supported",
            )),
            "gateway" if active.gateway.is_none() => active.gateway = Some(value.into()),
            "gateway" => unmodeled.push(NativeDirective::new(
                line_number,
                line,
                "multiple gateways in one stanza are not supported",
            )),
            "bridge-ports" => {
                if active.ports.is_some() {
                    unmodeled.push(NativeDirective::new(
                        line_number,
                        line,
                        "duplicate bridge-ports directive",
                    ));
                } else {
                    active.ports = Some(if value == "none" {
                        Vec::new()
                    } else {
                        value.split_whitespace().map(str::to_string).collect()
                    });
                }
            },
            "bridge-stp" if active.stp.is_some() => unmodeled.push(NativeDirective::new(
                line_number,
                line,
                "duplicate bridge-stp directive",
            )),
            "bridge-stp" => match value {
                "on" | "yes" | "1" => active.stp = Some(true),
                "off" | "no" | "0" => active.stp = Some(false),
                _ => unmodeled.push(NativeDirective::new(
                    line_number,
                    line,
                    "bridge-stp is not a supported boolean",
                )),
            },
            "bridge-fd" if active.forward_delay.is_some() => {
                unmodeled.push(NativeDirective::new(
                    line_number,
                    line,
                    "duplicate bridge-fd directive",
                ));
            },
            "bridge-fd" => match value.parse() {
                Ok(value) => active.forward_delay = Some(value),
                Err(_) => unmodeled.push(NativeDirective::new(
                    line_number,
                    line,
                    "bridge-fd is not a valid integer",
                )),
            },
            _ => unmodeled.push(NativeDirective::new(
                line_number,
                line,
                format!("unsupported interface directive `{key}`"),
            )),
        }
    }
    if let Some(previous) = stanza {
        stanzas.push(previous);
    }

    let mut defined = BTreeSet::new();
    for stanza in &stanzas {
        if !defined.insert((stanza.name.clone(), stanza.family.clone())) {
            unmodeled.push(NativeDirective::new(
                stanza.line,
                &stanza.definition,
                "multiple stanzas for the same interface and address family",
            ));
        }
    }
    let mut processed_inet = BTreeSet::new();
    for stanza in stanzas.iter().filter(|stanza| stanza.family == "inet") {
        if !processed_inet.insert(stanza.name.clone()) {
            continue;
        }
        if stanza.name == "lo" && stanza.method == "loopback" {
            continue;
        }
        if let Some(ports) = &stanza.ports {
            bridges.push(Bridge {
                name: stanza.name.clone(),
                method: stanza.method.clone(),
                address: stanza.address.clone(),
                gateway: stanza.gateway.clone(),
                ipv6: None,
                gateway6: None,
                ports: ports.clone(),
                stp: stanza.stp.unwrap_or(false),
                forward_delay: stanza.forward_delay.unwrap_or_default(),
            });
        } else if stanza.address.is_none()
            && stanza.gateway.is_none()
            && stanza.stp.is_none()
            && stanza.forward_delay.is_none()
        {
            interfaces.push(Interface {
                name: stanza.name.clone(),
                method: stanza.method.clone(),
            });
        } else {
            unmodeled.push(NativeDirective::new(
                stanza.line,
                &stanza.definition,
                "addressed non-bridge interfaces are not represented by the local model",
            ));
        }
    }
    let mut processed_inet6 = BTreeSet::new();
    for stanza in stanzas.iter().filter(|stanza| stanza.family == "inet6") {
        if !processed_inet6.insert(stanza.name.clone()) {
            continue;
        }
        if stanza.method != "static" {
            unmodeled.push(NativeDirective::new(
                stanza.line,
                &stanza.definition,
                "only static IPv6 bridge stanzas are supported",
            ));
            continue;
        }
        let Some(bridge) = bridges.iter_mut().find(|bridge| bridge.name == stanza.name) else {
            unmodeled.push(NativeDirective::new(
                stanza.line,
                &stanza.definition,
                "IPv6 configuration is only modeled for bridges",
            ));
            continue;
        };
        bridge.ipv6 = stanza.address.clone();
        bridge.gateway6 = stanza.gateway.clone();
    }
    for stanza in stanzas
        .iter()
        .filter(|stanza| stanza.family != "inet" && stanza.family != "inet6")
    {
        unmodeled.push(NativeDirective::new(
            stanza.line,
            &stanza.definition,
            format!("unsupported address family `{}`", stanza.family),
        ));
    }

    let reproducible_auto = bridges
        .iter()
        .map(|bridge| bridge.name.as_str())
        .chain(std::iter::once("lo"))
        .collect::<BTreeSet<_>>();
    for (line, name) in automatic {
        if !reproducible_auto.contains(name.as_str()) {
            unmodeled.push(NativeDirective::new(
                line,
                &format!("auto {name}"),
                "automatic activation for this interface is not represented by the local model",
            ));
        }
    }

    let mut adopted = current.clone();
    adopted.interfaces = interfaces;
    adopted.bridges = bridges;
    Ok(ParsedNetwork {
        managed: adopted,
        unmodeled,
    })
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

        assert!(parsed.unmodeled.is_empty());
        assert_eq!(parsed.managed.interfaces[0].name, "eno1");
        assert_eq!(parsed.managed.bridges[0].name, "vmbr0");
        assert_eq!(
            parsed.managed.bridges[0].ipv6.as_deref(),
            Some("2001:db8::10/64")
        );
        assert_eq!(
            parsed.managed.bridges[0].gateway6.as_deref(),
            Some("2001:db8::1")
        );
    }

    #[test]
    fn supported_network_is_stable_across_parse_and_render() {
        let current: Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let rendered = crate::render::network(&current);

        let first = parse_network(&rendered, &current).unwrap();
        assert!(first.unmodeled.is_empty());
        let rerendered = crate::render::network(&first.managed);
        let second = parse_network(&rerendered, &first.managed).unwrap();

        assert!(second.unmodeled.is_empty());
        assert_eq!(
            serde_json::to_value(&first.managed).unwrap(),
            serde_json::to_value(&second.managed).unwrap()
        );
    }

    #[test]
    fn unsupported_network_constructs_are_reported_instead_of_disappearing() {
        let current: Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let cases = [
            ("bridge-vlan-aware yes", "bridge-vlan-aware"),
            ("bond-slaves eno1 eno2", "bond-slaves"),
            ("mtu 9000", "mtu"),
            ("pre-up /usr/local/bin/prepare", "pre-up"),
            ("post-up /usr/local/bin/finish", "post-up"),
            ("dhclient-timeout 30", "dhclient-timeout"),
        ];

        for (directive, expected) in cases {
            let captured = format!("iface vmbr0 inet static\n    {directive}\n");
            let parsed = parse_network(&captured, &current).unwrap();
            assert!(
                parsed
                    .unmodeled
                    .iter()
                    .any(|item| item.text.contains(expected)),
                "missing blocker for {directive}"
            );
        }
    }

    #[test]
    fn multiple_addresses_and_custom_top_level_syntax_are_reported() {
        let current: Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let captured = "# keep this operator note\nallow-hotplug eno1\niface vmbr0 inet static\n    address 192.0.2.10/24\n    address 192.0.2.11/24\n    bridge-ports eno1\niface vmbr0 inet6 static\n    address 2001:db8::10/64\n    address 2001:db8::11/64\n";

        let parsed = parse_network(captured, &current).unwrap();
        let diagnostics = parsed
            .unmodeled
            .iter()
            .map(NativeDirective::diagnostic)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(diagnostics.contains("custom comments"));
        assert!(diagnostics.contains("unsupported top-level"));
        assert_eq!(diagnostics.matches("multiple addresses").count(), 2);
    }

    #[test]
    fn bare_dhcp_interface_round_trips_but_dhcp_options_do_not() {
        let current: Network =
            serde_yaml::from_str(include_str!("../../../examples/basic/config/network.yml"))
                .unwrap();
        let parsed = parse_network("iface eno1 inet dhcp\n", &current).unwrap();

        assert!(parsed.unmodeled.is_empty());
        assert_eq!(parsed.managed.interfaces[0].method, "dhcp");

        let with_option =
            parse_network("iface eno1 inet dhcp\n    metric 100\n", &current).unwrap();
        assert_eq!(with_option.unmodeled[0].text, "metric 100");
    }
}
