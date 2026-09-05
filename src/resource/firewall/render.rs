use crate::model::{FirewallPolicy, FirewallRule};

pub(crate) fn render(policy: &FirewallPolicy) -> String {
    let mut output = String::from("[OPTIONS]\n\n");
    output += &format!("enable: {}\n", u8::from(policy.enabled));
    if let Some(value) = &policy.log_level_in {
        output += &format!("log_level_in: {value}\n");
    }
    for (key, value) in &policy.options {
        output += &format!("{key}: {value}\n");
    }
    if !policy.aliases.is_empty() {
        output += "\n[ALIASES]\n\n";
        for alias in &policy.aliases {
            output += &format!("{} {}", alias.name, alias.network);
            comment(&mut output, alias.comment.as_deref());
        }
    }
    for set in &policy.ip_sets {
        output += &format!("\n[IPSET {}]\n\n", set.name);
        for entry in &set.entries {
            output += &entry.network;
            if entry.nomatch {
                output += " nomatch";
            }
            comment(&mut output, entry.comment.as_deref());
        }
    }
    for group in &policy.security_groups {
        output += &format!("\n[group {}]\n\n", group.name);
        rules(&mut output, &group.rules);
    }
    if !policy.rules.is_empty() {
        output += "\n[RULES]\n\n";
        rules(&mut output, &policy.rules);
    }
    output
}

fn rules(output: &mut String, rules: &[FirewallRule]) {
    for rule in rules {
        if !rule.enabled {
            output.push('|');
        }
        *output += &format!("{} ", rule.direction);
        if let Some(name) = &rule.macro_name {
            *output += &format!("{name}({})", rule.action);
        } else {
            *output += &rule.action.to_string();
        }
        if let Some(protocol) = &rule.protocol {
            *output += &format!(" -p {protocol}");
        }
        for (flag, value) in [
            ("-i", rule.interface.as_ref()),
            ("-source", rule.source.as_ref()),
            ("-dest", rule.destination.as_ref()),
            ("-sport", rule.source_port.as_ref()),
            ("-dport", rule.destination_port.as_ref()),
        ] {
            if let Some(value) = value {
                *output += &format!(" {flag} {value}");
            }
        }
        *output += &format!(" -log {}", rule.log);
        comment(output, rule.comment.as_deref());
    }
}
fn comment(output: &mut String, value: Option<&str>) {
    if let Some(value) = value {
        *output += &format!(" # {value}");
    }
    output.push('\n');
}

pub(crate) fn semantic(s: &str) -> (Vec<String>, Vec<String>) {
    let mut section = String::new();
    let mut unordered = Vec::new();
    let mut ordered = Vec::new();
    for line in crate::resource::network::render::semantic_lines(s) {
        if line.starts_with('[') {
            section.clone_from(&line);
            if line == "[RULES]" || line.starts_with("[group ") {
                ordered.push(line);
            }
        } else if section == "[OPTIONS]" || section == "[ALIASES]" || section.starts_with("[IPSET ")
        {
            unordered.push(format!("{section}:{line}"));
        } else if section == "[RULES]" || section.starts_with("[group ") {
            ordered.push(line);
        }
    }
    unordered.sort();
    (unordered, ordered)
}
