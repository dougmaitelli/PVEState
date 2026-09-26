use crate::reconcile::{ApiMethod, ApiTarget, Operation, Plan};
use console::style;
use std::collections::BTreeMap;

mod tags;
pub(crate) use tags::TagPalettes;

pub(super) fn has_guest_tags(operation: &Operation) -> bool {
    matches!(operation, Operation::ApiMutation { domain: crate::reconcile::Domain::Guest, changes, .. }
        if changes.contains_key("tags") || changes.get("delete").is_some_and(|value| value.split(',').any(|key| key == "tags")))
}

pub(crate) fn print_human(plan: &Plan, tag_palettes: &TagPalettes) {
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
            print_operation(operation, tag_palettes);
        }
    }
}

fn print_operation(operation: &Operation, tag_palettes: &TagPalettes) {
    match operation {
        Operation::ApiMutation {
            target,
            method,
            resource,
            endpoint,
            changes,
            before_values,
            environment_changes,
            ..
        } => {
            println!(
                "  {} {} {}",
                action(*method),
                resource,
                style(api_target(*target)).dim()
            );
            let lines = api_operation_lines(
                *method,
                changes,
                before_values,
                tag_palettes,
                console::colors_enabled(),
            );
            for line in lines {
                println!("      {line}");
            }
            for (field, variable) in environment_changes {
                println!(
                    "      {:<22} {}",
                    field,
                    style(environment_change(*method, variable.as_ref())).yellow()
                );
            }
            println!("      {}", style(endpoint).dim());
        },
        Operation::GrowDisk {
            resource,
            size_gb,
            before_size_gb,
            endpoint,
            ..
        } => {
            println!(
                "  {} {} {}",
                style("GROW").yellow().bold(),
                resource,
                disk_change(*before_size_gb, *size_gb)
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

fn disk_change(before: Option<u64>, after: u64) -> String {
    before.map_or_else(
        || format!("→ {after} GiB"),
        |before| format!("{before} GiB (captured) → {after} GiB (local)"),
    )
}

fn environment_change(method: ApiMethod, variable: &str) -> String {
    if method == ApiMethod::Post {
        format!("(unset) (captured) → from ${variable} (local)")
    } else {
        format!("from ${variable}")
    }
}

fn api_operation_lines(
    method: ApiMethod,
    changes: &BTreeMap<String, String>,
    before_values: &BTreeMap<String, Option<String>>,
    palettes: &TagPalettes,
    colors: bool,
) -> Vec<String> {
    if method == ApiMethod::Delete {
        return before_values
            .iter()
            .map(|(field, value)| {
                format!(
                    "{field:<22} {} (captured) → (removed) (local)",
                    captured_value(field, value, &palettes.captured, colors),
                )
            })
            .collect();
    }
    let unset;
    let before = if method == ApiMethod::Post {
        unset = changes.keys().map(|key| (key.clone(), None)).collect();
        &unset
    } else {
        before_values
    };
    if colors {
        api_change_lines_styled(changes, before, palettes, true)
    } else {
        api_change_lines(changes, before)
    }
}

pub(super) fn api_change_lines(
    changes: &BTreeMap<String, String>,
    before_values: &BTreeMap<String, Option<String>>,
) -> Vec<String> {
    api_change_lines_styled(changes, before_values, &TagPalettes::default(), false)
}

fn api_change_lines_styled(
    changes: &BTreeMap<String, String>,
    before_values: &BTreeMap<String, Option<String>>,
    tag_palettes: &TagPalettes,
    colors: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (field, value) in changes {
        if field == "delete" && value.split(',').all(|key| before_values.contains_key(key)) {
            for key in value.split(',') {
                lines.push(format!(
                    "{key:<22} {} (captured) → (removed) (local)",
                    captured_value(key, &before_values[key], &tag_palettes.captured, colors),
                ));
            }
        } else if let Some(before) = before_values.get(field) {
            lines.push(format!(
                "{field:<22} {} (captured) → {} (local)",
                captured_value(field, before, &tag_palettes.captured, colors),
                tags::display_value(field, value, &tag_palettes.local, colors),
            ));
        } else {
            lines.push(format!(
                "{field:<22} {}",
                tags::display_value(field, value, &tag_palettes.local, colors)
            ));
        }
    }
    lines
}

fn captured_value(
    field: &str,
    value: &Option<String>,
    palette: &crate::resource::tag_colors::TagColors,
    colors: bool,
) -> String {
    value.as_deref().map_or_else(
        || "(unset)".into(),
        |value| tags::display_value(field, value, palette, colors),
    )
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
    if domain == crate::reconcile::Domain::Network.as_str() {
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
    #[test]
    fn resource_creation_deletion_and_growth_show_both_sides() {
        use super::*;
        let changes = BTreeMap::from([("schedule".into(), "daily".into())]);
        let before = BTreeMap::from([("schedule".into(), Some("weekly".into()))]);
        let palettes = TagPalettes::default();
        let created = api_operation_lines(
            ApiMethod::Post,
            &changes,
            &BTreeMap::new(),
            &palettes,
            false,
        );
        assert_eq!(
            created,
            [format!(
                "{:<22} (unset) (captured) → daily (local)",
                "schedule"
            )]
        );
        let deleted = api_operation_lines(
            ApiMethod::Delete,
            &BTreeMap::new(),
            &before,
            &palettes,
            false,
        );
        assert_eq!(
            deleted,
            [format!(
                "{:<22} weekly (captured) → (removed) (local)",
                "schedule"
            )]
        );
        assert_eq!(
            environment_change(ApiMethod::Post, "SECRET_KEY"),
            "(unset) (captured) → from $SECRET_KEY (local)"
        );
        assert_eq!(
            disk_change(Some(32), 64),
            "32 GiB (captured) → 64 GiB (local)"
        );
        assert_eq!(disk_change(None, 64), "→ 64 GiB");
    }

    use super::*;

    #[test]
    fn tag_diffs_use_each_sides_palette_including_removals() {
        let palettes = TagPalettes {
            captured: serde_json::from_value(
                serde_json::json!({"web": {"background": "ff0000", "text": "ffffff"}}),
            )
            .unwrap(),
            local: serde_json::from_value(
                serde_json::json!({"web": {"background": "0000ff", "text": "ffffff"}}),
            )
            .unwrap(),
        };
        let before = BTreeMap::from([("tags".into(), Some("web".into()))]);
        for changes in [
            BTreeMap::from([("tags".into(), "web;db".into())]),
            BTreeMap::from([("delete".into(), "tags".into())]),
        ] {
            let rendered = api_change_lines_styled(&changes, &before, &palettes, true).join("\n");
            let (captured, local) = rendered.split_once(" → ").unwrap();
            assert!(captured.contains("\x1b[48;2;255;0;0mweb\x1b[0m"));
            assert!(!local.contains("\x1b[48;2;255;0;0m"));
            if changes.contains_key("tags") {
                assert!(local.contains("\x1b[48;2;0;0;255mweb\x1b[0m"));
            } else {
                assert_eq!(local, "(removed) (local)");
            }
            let plain = api_change_lines_styled(&changes, &before, &palettes, false).join("\n");
            assert_eq!(console::strip_ansi_codes(&rendered), plain);
            assert!(!plain.contains('\x1b'));
        }
    }

    #[test]
    fn api_changes_distinguish_unset_empty_and_unknown_values() {
        let changes = BTreeMap::from([
            ("search".into(), "example.test".into()),
            ("dns1".into(), "192.0.2.53".into()),
            ("description".into(), "".into()),
        ]);
        let before = BTreeMap::from([
            ("search".into(), Some(String::new())),
            ("dns1".into(), None),
        ]);
        let lines = api_change_lines(&changes, &before).join("\n");
        assert!(lines.contains("(empty) (captured) → example.test (local)"));
        assert!(lines.contains("(unset) (captured) → 192.0.2.53 (local)"));
        assert!(lines.contains(&format!("{:<22} (empty)", "description")));
    }

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
