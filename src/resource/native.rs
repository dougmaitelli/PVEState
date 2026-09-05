use crate::resource::native_paths;
use crate::utility::yaml_patch::Segment;
use crate::{
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedNative,
    model::{
        Bridge, FirewallAction, FirewallAlias, FirewallDirection, FirewallIpSet,
        FirewallIpSetEntry, FirewallLogLevel, FirewallPolicy, FirewallProtocol, FirewallRule,
        FirewallSecurityGroup, Interface, Network,
    },
    reconcile::{Domain, Operation},
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(crate) struct ParsedNetwork {
    pub(crate) managed: Network,
    pub(crate) unmodeled: Vec<NativeDirective>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeDirective {
    pub(crate) line: usize,
    pub(crate) text: String,
    pub(crate) reason: String,
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
pub(crate) enum Target {
    Network,
    Firewall { resource: String },
}

pub(crate) fn adoption_patches(
    local: &LocalState,
    captured: &CapturedNative,
    target: &Target,
) -> Result<Vec<LocalPatch>> {
    match target {
        Target::Network => {
            let content = captured.read_to_string(native_paths::NETWORK_ARTIFACT)?;
            let parsed = parse_network(&content, &local.network)?;
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
            Ok(vec![
                replace(
                    ConfigDocument::Network,
                    vec![Segment::Key("interfaces".into())],
                    &parsed.managed.interfaces,
                )?,
                replace(
                    ConfigDocument::Network,
                    vec![Segment::Key("bridges".into())],
                    &parsed.managed.bridges,
                )?,
            ])
        },
        Target::Firewall { resource } => {
            let (observed, document, path) = firewall_paths(local, captured, resource)?;
            let policy = if let Ok(content) = std::fs::read_to_string(observed) {
                let parsed = parse_firewall(&content);
                if !parsed.unmodeled.is_empty() {
                    bail!(
                        "captured firewall contains syntax that cannot be adopted safely: {}",
                        parsed
                            .unmodeled
                            .iter()
                            .map(NativeDirective::diagnostic)
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                }
                Some(parsed.managed)
            } else {
                None
            };
            Ok(vec![if let Some(policy) = policy {
                LocalPatch::ReplaceResource {
                    document,
                    path,
                    value: serde_yaml::to_value(policy)?,
                }
            } else {
                LocalPatch::RemoveResource { document, path }
            }])
        },
    }
}

fn replace(
    document: ConfigDocument,
    path: Vec<Segment>,
    value: &impl serde::Serialize,
) -> Result<LocalPatch> {
    Ok(LocalPatch::ReplaceResource {
        document,
        path,
        value: serde_yaml::to_value(value)?,
    })
}

fn firewall_paths(
    local: &LocalState,
    captured: &CapturedNative,
    resource: &str,
) -> Result<(std::path::PathBuf, ConfigDocument, Vec<Segment>)> {
    let observed = captured.path(native_paths::FIREWALL_ARTIFACT_ROOT);
    if resource == "cluster" {
        return Ok((
            observed.join("cluster.fw"),
            ConfigDocument::Cluster,
            vec![Segment::Key("firewall".into())],
        ));
    }
    if let Some(node) = resource.strip_prefix("node/") {
        return Ok((
            observed.join(format!("{node}-host.fw")),
            ConfigDocument::Node,
            vec![Segment::Key("firewall".into())],
        ));
    }
    let id: u32 = resource
        .parse()
        .with_context(|| format!("invalid firewall resource {resource}"))?;
    let (collection, exists) = if local.guests.lxcs.contains_key(&id) {
        ("lxcs", true)
    } else {
        ("vms", local.guests.vms.contains_key(&id))
    };
    if !exists {
        bail!("firewall resource {resource} is not a configured guest")
    }
    Ok((
        observed.join(format!("{id}.fw")),
        ConfigDocument::Guests,
        vec![
            Segment::Key(collection.into()),
            Segment::Key(id.to_string()),
            Segment::Key("firewall".into()),
        ],
    ))
}

pub(crate) struct ParsedFirewall {
    pub(crate) managed: FirewallPolicy,
    pub(crate) unmodeled: Vec<NativeDirective>,
}

pub(crate) fn parse_firewall(content: &str) -> ParsedFirewall {
    enum Section {
        None,
        Options,
        Aliases,
        IpSet(usize),
        Group(usize),
        Rules,
        Unknown,
    }

    let mut section = Section::None;
    let mut enabled = false;
    let mut log_level_in = None;
    let mut options = std::collections::BTreeMap::new();
    let mut aliases = Vec::new();
    let mut ip_sets: Vec<FirewallIpSet> = Vec::new();
    let mut security_groups: Vec<FirewallSecurityGroup> = Vec::new();
    let mut rules = Vec::new();
    let mut unmodeled = Vec::new();

    for (index, raw) in content.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            unmodeled.push(NativeDirective::new(
                number,
                line,
                "standalone comments are not represented by the local firewall model",
            ));
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = match line {
                "[OPTIONS]" => Section::Options,
                "[ALIASES]" => Section::Aliases,
                "[RULES]" => Section::Rules,
                _ if line.starts_with("[IPSET ") => {
                    let name = line[7..line.len() - 1].trim();
                    if name.is_empty() {
                        unmodeled.push(NativeDirective::new(number, line, "IP set has no name"));
                        Section::Unknown
                    } else {
                        ip_sets.push(FirewallIpSet {
                            name: name.into(),
                            entries: Vec::new(),
                        });
                        Section::IpSet(ip_sets.len() - 1)
                    }
                },
                _ if line.starts_with("[group ") => {
                    let name = line[7..line.len() - 1].trim();
                    if name.is_empty() {
                        unmodeled.push(NativeDirective::new(
                            number,
                            line,
                            "security group has no name",
                        ));
                        Section::Unknown
                    } else {
                        security_groups.push(FirewallSecurityGroup {
                            name: name.into(),
                            rules: Vec::new(),
                        });
                        Section::Group(security_groups.len() - 1)
                    }
                },
                _ => {
                    unmodeled.push(NativeDirective::new(
                        number,
                        line,
                        "unsupported firewall section",
                    ));
                    Section::Unknown
                },
            };
            continue;
        }
        if matches!(section, Section::Options) {
            if let Some((key, value)) = line.split_once(':') {
                match key.trim() {
                    "enable" => enabled = matches!(value.trim(), "1" | "yes" | "true" | "on"),
                    "log_level_in" => match value.trim().parse::<FirewallLogLevel>() {
                        Ok(value) => log_level_in = Some(value),
                        Err(error) => {
                            unmodeled.push(NativeDirective::new(number, line, error.to_string()))
                        },
                    },
                    key => {
                        if options.insert(key.into(), value.trim().into()).is_some() {
                            unmodeled.push(NativeDirective::new(
                                number,
                                line,
                                format!("duplicate firewall option `{key}`"),
                            ));
                        }
                    },
                }
            } else {
                unmodeled.push(NativeDirective::new(
                    number,
                    line,
                    "malformed firewall option",
                ));
            }
        } else if matches!(section, Section::Aliases) {
            match parse_alias(line) {
                Some(alias) => aliases.push(alias),
                None => unmodeled.push(NativeDirective::new(number, line, "malformed alias")),
            }
        } else if let Section::IpSet(position) = section {
            match parse_ip_set_entry(line) {
                Some(entry) => ip_sets[position].entries.push(entry),
                None => {
                    unmodeled.push(NativeDirective::new(number, line, "malformed IP set entry"))
                },
            }
        } else if matches!(section, Section::Rules) {
            match parse_firewall_rule(line) {
                Ok(rule) => rules.push(rule),
                Err(error) => unmodeled.push(NativeDirective::new(number, line, error.to_string())),
            }
        } else if let Section::Group(position) = section {
            match parse_firewall_rule(line) {
                Ok(rule) => security_groups[position].rules.push(rule),
                Err(error) => unmodeled.push(NativeDirective::new(number, line, error.to_string())),
            }
        } else if !matches!(section, Section::Unknown) {
            unmodeled.push(NativeDirective::new(
                number,
                line,
                "firewall content appears outside a supported section",
            ));
        }
    }

    ParsedFirewall {
        managed: FirewallPolicy {
            enabled,
            log_level_in,
            options,
            aliases,
            ip_sets,
            security_groups,
            rules,
        },
        unmodeled,
    }
}

fn split_comment(line: &str) -> (&str, Option<String>) {
    line.split_once('#')
        .map_or((line, None), |(value, comment)| {
            (value.trim_end(), Some(comment.trim().to_string()))
        })
}

fn parse_alias(line: &str) -> Option<FirewallAlias> {
    let (value, comment) = split_comment(line);
    let mut words = value.split_whitespace();
    let name = words.next()?.to_string();
    let network = words.next()?.to_string();
    (words.next().is_none()).then_some(FirewallAlias {
        name,
        network,
        comment,
    })
}

fn parse_ip_set_entry(line: &str) -> Option<FirewallIpSetEntry> {
    let (value, comment) = split_comment(line);
    let mut words = value.split_whitespace();
    let network = words.next()?.to_string();
    let option = words.next();
    if option.is_some_and(|value| value != "nomatch") || words.next().is_some() {
        return None;
    }
    Some(FirewallIpSetEntry {
        network,
        nomatch: option.is_some(),
        comment,
    })
}

fn parse_firewall_rule(line: &str) -> Result<FirewallRule> {
    let (enabled, line) = line
        .strip_prefix('|')
        .map_or((true, line), |line| (false, line));
    let (rule, comment) = split_comment(line);
    let mut words = rule.split_whitespace();
    let direction: FirewallDirection = words.next().context("firewall rule direction")?.parse()?;
    let action_token = words.next().context("firewall rule action")?;
    let (macro_name, action) = if let Some((name, action)) = action_token.split_once('(') {
        (
            Some(name.to_string()),
            action
                .strip_suffix(')')
                .context("malformed firewall macro action")?
                .parse::<FirewallAction>()?,
        )
    } else {
        (None, action_token.parse()?)
    };
    let mut interface = None;
    let mut protocol = None;
    let mut source = None;
    let mut destination = None;
    let mut source_port = None;
    let mut destination_port = None;
    let mut log = FirewallLogLevel::NoLog;
    while let Some(flag) = words.next() {
        let value = words
            .next()
            .with_context(|| format!("firewall rule value after {flag}"))?;
        match flag {
            "-i" => interface = Some(value.into()),
            "-p" => protocol = Some(value.parse::<FirewallProtocol>()?),
            "-source" => source = Some(value.into()),
            "-dest" => destination = Some(value.into()),
            "-sport" => source_port = Some(value.into()),
            "-dport" => destination_port = Some(value.into()),
            "-log" => log = value.parse()?,
            _ => bail!("unsupported captured firewall rule option {flag}"),
        }
    }
    Ok(FirewallRule {
        enabled,
        direction,
        action,
        macro_name,
        interface,
        protocol,
        source,
        destination,
        source_port,
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
            let Ok(method) = stanza.method.parse() else {
                unmodeled.push(NativeDirective::new(
                    stanza.line,
                    &stanza.definition,
                    format!("unsupported IPv4 address method `{}`", stanza.method),
                ));
                continue;
            };
            bridges.push(Bridge {
                name: stanza.name.clone(),
                method,
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
            let Ok(method) = stanza.method.parse() else {
                unmodeled.push(NativeDirective::new(
                    stanza.line,
                    &stanza.definition,
                    format!("unsupported IPv4 address method `{}`", stanza.method),
                ));
                continue;
            };
            interfaces.push(Interface {
                name: stanza.name.clone(),
                method,
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

pub(crate) fn adoption_candidate(
    local: &LocalState,
    captured: &crate::discovery::CapturedNative,
    operation: &Operation,
) -> Result<AdoptionCandidate> {
    let target = match operation.domain() {
        Domain::Network => Target::Network,
        Domain::Firewall => Target::Firewall {
            resource: operation.resource().to_string(),
        },
        domain => {
            return Ok(AdoptionCandidate::blocked(
                operation.resource(),
                "file",
                "local rendering",
                "captured file",
                format!("{domain} has no native adoption adapter"),
            ));
        },
    };
    match adoption_patches(local, captured, &target) {
        Ok(patches) => Ok(AdoptionCandidate::adoptable(
            operation.resource(),
            "file",
            "local rendering",
            "captured file",
            patches,
        )),
        Err(error) => Ok(AdoptionCandidate::blocked(
            operation.resource(),
            "file",
            "local rendering",
            "captured file",
            format!("{error:#}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_disabled_firewall_rules() {
        let parsed = parse_firewall(
            "[OPTIONS]\nenable: 1\n\n[RULES]\n|IN ACCEPT -p tcp -dport 443 -log nolog # Web\n",
        );
        assert!(parsed.unmodeled.is_empty());
        let policy = parsed.managed;
        assert!(policy.enabled);
        assert!(!policy.rules[0].enabled);
        assert_eq!(policy.rules[0].destination_port.as_deref(), Some("443"));
        assert_eq!(policy.rules[0].comment.as_deref(), Some("Web"));
    }

    #[test]
    fn firewall_sections_and_rule_vocabulary_round_trip() {
        let captured = "[OPTIONS]\n\nenable: 1\nlog_level_in: info\npolicy_in: DROP\n\n[ALIASES]\n\nadmin 192.0.2.10 # Administrator\n\n[IPSET trusted]\n\n192.0.2.0/24\n198.51.100.10 nomatch # Exception\n\n[group web]\n\nIN HTTPS(ACCEPT) -source +trusted -dest 203.0.113.10 -sport 1024:65535 -dport 443 -log info # Web\n\n[RULES]\n\n|IN SSH(ACCEPT) -i vmbr0 -p tcp -source admin -dest 192.0.2.20 -sport 1024:65535 -dport 22 -log nolog # Admin SSH\n";
        let parsed = parse_firewall(captured);

        assert!(parsed.unmodeled.is_empty());
        assert_eq!(parsed.managed.options["policy_in"], "DROP");
        assert_eq!(parsed.managed.aliases[0].name, "admin");
        assert!(parsed.managed.ip_sets[0].entries[1].nomatch);
        assert_eq!(parsed.managed.security_groups[0].name, "web");
        assert_eq!(parsed.managed.rules[0].macro_name.as_deref(), Some("SSH"));
        assert_eq!(parsed.managed.rules[0].source.as_deref(), Some("admin"));
        assert!(!parsed.managed.rules[0].enabled);

        let rendered = crate::resource::firewall::render::render(&parsed.managed);
        let reparsed = parse_firewall(&rendered);
        assert!(reparsed.unmodeled.is_empty());
        assert_eq!(
            serde_json::to_value(parsed.managed).unwrap(),
            serde_json::to_value(reparsed.managed).unwrap()
        );
    }

    #[test]
    fn unknown_firewall_sections_and_flags_are_reported() {
        let parsed = parse_firewall(
            "# preserve me\n[UNKNOWN]\nvalue\n\n[RULES]\nIN ACCEPT -p tcp -m conntrack -dport 443\n",
        );
        let diagnostics = parsed
            .unmodeled
            .iter()
            .map(NativeDirective::diagnostic)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(diagnostics.contains("unsupported firewall section"));
        assert!(diagnostics.contains("unsupported captured firewall rule option -m"));
        assert!(diagnostics.contains("standalone comments"));
    }

    #[test]
    fn parses_ipv4_and_ipv6_bridge_configuration() {
        let current: Network =
            serde_yaml::from_str(include_str!("../../examples/basic/config/network.yml")).unwrap();
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
            serde_yaml::from_str(include_str!("../../examples/basic/config/network.yml")).unwrap();
        let rendered = crate::resource::network::render::render(&current);

        let first = parse_network(&rendered, &current).unwrap();
        assert!(first.unmodeled.is_empty());
        let rerendered = crate::resource::network::render::render(&first.managed);
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
            serde_yaml::from_str(include_str!("../../examples/basic/config/network.yml")).unwrap();
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
            serde_yaml::from_str(include_str!("../../examples/basic/config/network.yml")).unwrap();
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
            serde_yaml::from_str(include_str!("../../examples/basic/config/network.yml")).unwrap();
        let parsed = parse_network("iface eno1 inet dhcp\n", &current).unwrap();

        assert!(parsed.unmodeled.is_empty());
        assert_eq!(parsed.managed.interfaces[0].method, "dhcp");

        let with_option =
            parse_network("iface eno1 inet dhcp\n    metric 100\n", &current).unwrap();
        assert_eq!(with_option.unmodeled[0].text, "metric 100");
    }
}
