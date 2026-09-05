use super::{ApiMethod, ApiTarget, Operation, Plan};
use console::style;
use std::collections::BTreeMap;

pub(crate) fn print_human(plan: &Plan) {
    println!("{}", style("Live change plan").bold().cyan());
    println!("  {:<10} {}", "PVE", plan.target);
    println!("  {:<10} {}", "PBS", plan.pbs_target);
    println!("  {:<10} {}", "Created", plan.created_at.to_rfc3339());
    println!("  {:<10} {}", "Capture", plan.capture_id);
    println!("  {:<10} {}", "SHA-256", plan.plan_sha256);

    if !plan.blockers.is_empty() {
        println!(
            "\n{}",
            style(format!("Blockers ({})", plan.blockers.len()))
                .red()
                .bold()
        );
        for blocker in &plan.blockers {
            println!("  {} {blocker}", style("!").red().bold());
        }
    }

    if plan.operations.is_empty() {
        println!("\n{}", style("No changes").green().bold());
        return;
    }

    let mut domains: BTreeMap<&str, Vec<&Operation>> = BTreeMap::new();
    for operation in &plan.operations {
        domains
            .entry(operation.domain().as_str())
            .or_default()
            .push(operation);
    }
    println!(
        "\n{}",
        style(format!("Changes ({})", plan.operations.len())).bold()
    );
    for (domain, operations) in domains {
        println!(
            "\n{} {}",
            style(domain.to_uppercase()).cyan().bold(),
            style(format!("({})", operations.len())).dim()
        );
        for operation in operations {
            print_operation(operation);
        }
    }
}

fn print_operation(operation: &Operation) {
    match operation {
        Operation::ApiMutation {
            target,
            method,
            resource,
            endpoint,
            changes,
            environment_changes,
            ..
        } => {
            println!(
                "  {} {} {}",
                action(*method),
                resource,
                style(api_target(*target)).dim()
            );
            for (field, value) in changes {
                println!("      {:<22} {}", field, display_value(field, value));
            }
            for (field, variable) in environment_changes {
                println!(
                    "      {:<22} {}",
                    field,
                    style(format!("from ${variable}")).yellow()
                );
            }
            println!("      {}", style(endpoint).dim());
        },
        Operation::GrowDisk {
            resource,
            size_gb,
            endpoint,
            ..
        } => {
            println!(
                "  {} {} → {} GiB",
                style("GROW").yellow().bold(),
                resource,
                size_gb
            );
            println!("      {}", style(endpoint).dim());
        },
        Operation::WriteFile {
            domain,
            resource,
            target,
            content,
            before_content,
            ..
        } => {
            println!("  {} {resource}", style("WRITE").green().bold());
            println!("      {}", target.path());
            print_diff(
                domain.as_str(),
                before_content.as_deref().unwrap_or_default(),
                content,
            );
            if target.requires_activation() {
                println!("      {}", style("requires activation").yellow());
            }
        },
        Operation::DeleteFile {
            resource,
            target,
            domain,
            before_content,
            ..
        } => {
            println!("  {} {resource}", style("DELETE").red().bold());
            println!("      {}", target.path());
            print_diff(domain.as_str(), before_content, "");
        },
    }
}

fn action(method: ApiMethod) -> console::StyledObject<&'static str> {
    match method {
        ApiMethod::Post => style("CREATE").green().bold(),
        ApiMethod::Put => style("UPDATE").yellow().bold(),
        ApiMethod::Delete => style("DELETE").red().bold(),
    }
}

const fn api_target(target: ApiTarget) -> &'static str {
    match target {
        ApiTarget::Pve => "PVE API",
        ApiTarget::Pbs => "PBS API",
    }
}

fn display_value(field: &str, value: &str) -> String {
    if field == "delete" {
        format!("remove {value}")
    } else if value.is_empty() {
        "(empty)".into()
    } else {
        value.into()
    }
}

#[derive(Debug, PartialEq)]
enum DiffLine<'a> {
    Context(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

fn print_diff(domain: &str, before: &str, after: &str) {
    let (before, after) = comparable_content(domain, before, after);
    println!("      {}", style("--- captured").red().dim());
    println!("      {}", style("+++ local").green().dim());

    let diff = line_diff(&before, &after);
    let changed = diff
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (!matches!(line, DiffLine::Context(_))).then_some(index))
        .collect::<Vec<_>>();
    let mut skipped = false;

    for (index, line) in diff.iter().enumerate() {
        let visible = !matches!(line, DiffLine::Context(_))
            || changed.iter().any(|change| index.abs_diff(*change) <= 2);
        if !visible {
            skipped = true;
            continue;
        }
        if skipped {
            println!("      {}", style("…").dim());
            skipped = false;
        }
        match line {
            DiffLine::Context(value) => println!("       {value}"),
            DiffLine::Removed(value) => println!("      {}", style(format!("-{value}")).red()),
            DiffLine::Added(value) => println!("      {}", style(format!("+{value}")).green()),
        }
    }
    if skipped {
        println!("      {}", style("…").dim());
    }
}

fn comparable_content(domain: &str, before: &str, after: &str) -> (String, String) {
    if domain == crate::command::plan::Domain::Network.as_str() {
        (
            crate::resource::network::render::semantic_lines(before).join("\n"),
            crate::resource::network::render::semantic_lines(after).join("\n"),
        )
    } else {
        (before.into(), after.into())
    }
}

fn line_diff<'a>(before: &'a str, after: &'a str) -> Vec<DiffLine<'a>> {
    let before = before.lines().collect::<Vec<_>>();
    let after = after.lines().collect::<Vec<_>>();
    let mut lengths = vec![vec![0_usize; after.len() + 1]; before.len() + 1];

    for old in (0..before.len()).rev() {
        for new in (0..after.len()).rev() {
            lengths[old][new] = if before[old] == after[new] {
                lengths[old + 1][new + 1] + 1
            } else {
                lengths[old + 1][new].max(lengths[old][new + 1])
            };
        }
    }

    let mut result = Vec::new();
    let (mut old, mut new) = (0, 0);
    while old < before.len() || new < after.len() {
        if old < before.len() && new < after.len() && before[old] == after[new] {
            result.push(DiffLine::Context(before[old]));
            old += 1;
            new += 1;
        } else if new < after.len()
            && (old == before.len() || lengths[old][new + 1] > lengths[old + 1][new])
        {
            result.push(DiffLine::Added(after[new]));
            new += 1;
        } else {
            result.push(DiffLine::Removed(before[old]));
            old += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_removed_and_empty_values_for_people() {
        assert_eq!(
            display_value("delete", "memory,cores"),
            "remove memory,cores"
        );
        assert_eq!(display_value("description", ""), "(empty)");
        assert_eq!(display_value("memory", "4096"), "4096");
    }

    #[test]
    fn calculates_an_ordered_line_diff() {
        assert_eq!(
            line_diff(
                "auto lo\naddress 192.0.2.1\n",
                "auto lo\naddress 192.0.2.2\n"
            ),
            vec![
                DiffLine::Context("auto lo"),
                DiffLine::Removed("address 192.0.2.1"),
                DiffLine::Added("address 192.0.2.2"),
            ]
        );
    }

    #[test]
    fn network_diff_ignores_generated_comments_and_indentation() {
        let before = "# generated by PVE\niface vmbr0 inet static\n\taddress 192.0.2.10/24\n";
        let after = "# Managed by PVE State\niface vmbr0 inet static\n    address 192.0.2.10/24\n";

        let (before, after) = comparable_content("network", before, after);

        assert_eq!(before, after);
        assert_eq!(before, "iface vmbr0 inet static\naddress 192.0.2.10/24");
    }

    #[test]
    fn non_network_diff_remains_exact() {
        let before = "# old\n value\n";
        let after = "# new\nvalue\n";

        assert_eq!(
            comparable_content("firewall", before, after),
            (before.into(), after.into())
        );
    }
}
