use super::{
    FirewallAction, FirewallAlias, FirewallDirection, FirewallIpSet, FirewallIpSetEntry,
    FirewallLogLevel, FirewallPolicy, FirewallProtocol, FirewallRule, FirewallSecurityGroup,
};
use crate::resource::native::NativeDirective;
use anyhow::{Context, Result, bail};

pub(crate) struct ParsedFirewall {
    pub(crate) managed: FirewallPolicy,
    pub(crate) unmodeled: Vec<NativeDirective>,
}

pub(crate) fn parse(content: &str) -> ParsedFirewall {
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
                    named_section(line, 7, number, "IP set", &mut unmodeled).map_or(
                        Section::Unknown,
                        |name| {
                            ip_sets.push(FirewallIpSet {
                                name,
                                entries: Vec::new(),
                            });
                            Section::IpSet(ip_sets.len() - 1)
                        },
                    )
                },
                _ if line.starts_with("[group ") => {
                    named_section(line, 7, number, "security group", &mut unmodeled).map_or(
                        Section::Unknown,
                        |name| {
                            security_groups.push(FirewallSecurityGroup {
                                name,
                                rules: Vec::new(),
                            });
                            Section::Group(security_groups.len() - 1)
                        },
                    )
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
        match section {
            Section::Options => parse_option(
                line,
                number,
                &mut enabled,
                &mut log_level_in,
                &mut options,
                &mut unmodeled,
            ),
            Section::Aliases => match parse_alias(line) {
                Some(alias) => aliases.push(alias),
                None => unmodeled.push(NativeDirective::new(number, line, "malformed alias")),
            },
            Section::IpSet(position) => match parse_ip_set_entry(line) {
                Some(entry) => ip_sets[position].entries.push(entry),
                None => {
                    unmodeled.push(NativeDirective::new(number, line, "malformed IP set entry"))
                },
            },
            Section::Rules => parse_rule_into(line, number, &mut rules, &mut unmodeled),
            Section::Group(position) => parse_rule_into(
                line,
                number,
                &mut security_groups[position].rules,
                &mut unmodeled,
            ),
            Section::Unknown => {},
            Section::None => unmodeled.push(NativeDirective::new(
                number,
                line,
                "firewall content appears outside a supported section",
            )),
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

fn named_section(
    line: &str,
    prefix: usize,
    number: usize,
    kind: &str,
    unmodeled: &mut Vec<NativeDirective>,
) -> Option<String> {
    let name = line[prefix..line.len() - 1].trim();
    if name.is_empty() {
        unmodeled.push(NativeDirective::new(
            number,
            line,
            format!("{kind} has no name"),
        ));
        None
    } else {
        Some(name.into())
    }
}

fn parse_option(
    line: &str,
    number: usize,
    enabled: &mut bool,
    log_level_in: &mut Option<FirewallLogLevel>,
    options: &mut std::collections::BTreeMap<String, String>,
    unmodeled: &mut Vec<NativeDirective>,
) {
    let Some((key, value)) = line.split_once(':') else {
        unmodeled.push(NativeDirective::new(
            number,
            line,
            "malformed firewall option",
        ));
        return;
    };
    let key = key.trim();
    let value = value.trim();
    match key {
        "enable" => *enabled = matches!(value, "1" | "yes" | "true" | "on"),
        "log_level_in" => match value.parse::<FirewallLogLevel>() {
            Ok(value) => *log_level_in = Some(value),
            Err(error) => unmodeled.push(NativeDirective::new(number, line, error.to_string())),
        },
        key => {
            if options.insert(key.into(), value.into()).is_some() {
                unmodeled.push(NativeDirective::new(
                    number,
                    line,
                    format!("duplicate firewall option `{key}`"),
                ));
            }
        },
    }
}

fn parse_rule_into(
    line: &str,
    number: usize,
    rules: &mut Vec<FirewallRule>,
    unmodeled: &mut Vec<NativeDirective>,
) {
    match parse_rule(line) {
        Ok(rule) => rules.push(rule),
        Err(error) => unmodeled.push(NativeDirective::new(number, line, error.to_string())),
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

pub(super) fn parse_rule(line: &str) -> Result<FirewallRule> {
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
    let mut parsed = RuleOptions::default();
    while let Some(flag) = words.next() {
        let value = words
            .next()
            .with_context(|| format!("firewall rule value after {flag}"))?;
        parsed.assign(flag, value)?;
    }
    Ok(FirewallRule {
        enabled,
        direction,
        action,
        macro_name,
        interface: parsed.interface,
        protocol: parsed.protocol,
        source: parsed.source,
        destination: parsed.destination,
        source_port: parsed.source_port,
        destination_port: parsed.destination_port,
        log: parsed.log.unwrap_or(FirewallLogLevel::NoLog),
        comment,
    })
}

#[derive(Default)]
struct RuleOptions {
    interface: Option<String>,
    protocol: Option<FirewallProtocol>,
    source: Option<String>,
    destination: Option<String>,
    source_port: Option<String>,
    destination_port: Option<String>,
    log: Option<FirewallLogLevel>,
}

impl RuleOptions {
    fn assign(&mut self, flag: &str, value: &str) -> Result<()> {
        match flag {
            "-i" => self.interface = Some(value.into()),
            "-p" => self.protocol = Some(value.parse()?),
            "-source" => self.source = Some(value.into()),
            "-dest" => self.destination = Some(value.into()),
            "-sport" => self.source_port = Some(value.into()),
            "-dport" => self.destination_port = Some(value.into()),
            "-log" => self.log = Some(value.parse()?),
            _ => bail!("unsupported captured firewall rule option {flag}"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_render_parse_preserves_supported_crlf_input() {
        let captured = "[OPTIONS]\r\nenable: 1\r\nlog_level_in: info\r\npolicy_in: DROP\r\n\r\n[ALIASES]\r\nadmin 192.0.2.10 # Administrator\r\n\r\n[IPSET trusted]\r\n192.0.2.0/24\r\n\r\n[group web]\r\nIN HTTPS(ACCEPT) -source +trusted -dport 443 -log info\r\n\r\n[RULES]\r\n|IN SSH(ACCEPT) -i vmbr0 -p tcp -source admin -dport 22 -log nolog # SSH\r\n";
        let first = parse(captured);
        assert!(first.unmodeled.is_empty());
        assert!(!first.managed.rules[0].enabled);
        let rendered = super::super::render::render(&first.managed);
        let second = parse(&rendered);
        assert!(second.unmodeled.is_empty());
        assert_eq!(
            serde_json::to_value(first.managed).unwrap(),
            serde_json::to_value(second.managed).unwrap()
        );
    }

    #[test]
    fn mixed_supported_and_unsupported_sections_are_reported() {
        let parsed = parse(
            "[OPTIONS]\nenable: 1\n\n[UNKNOWN]\nvalue\n\n[RULES]\nIN ACCEPT -p tcp -m conntrack -dport 443\n",
        );
        assert!(parsed.managed.enabled);
        let diagnostics = parsed
            .unmodeled
            .iter()
            .map(NativeDirective::diagnostic)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(diagnostics.contains("unsupported firewall section"));
        assert!(diagnostics.contains("unsupported captured firewall rule option -m"));
    }

    #[test]
    fn disabled_rules_retain_ports_and_comments() {
        let parsed = parse("[RULES]\n|IN ACCEPT -p tcp -dport 443 -log nolog # Web\n");
        assert!(parsed.unmodeled.is_empty());
        assert!(!parsed.managed.rules[0].enabled);
        assert_eq!(
            parsed.managed.rules[0].destination_port.as_deref(),
            Some("443")
        );
        assert_eq!(parsed.managed.rules[0].comment.as_deref(), Some("Web"));
    }
}
