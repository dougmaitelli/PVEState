use anyhow::{Context, Result, bail};
use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone)]
pub(crate) enum Segment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone)]
pub(crate) enum Patch {
    Set(Vec<Segment>, Value),
    #[allow(dead_code, reason = "used by collection adoption adapters")]
    Remove(Vec<Segment>),
}

/// Applies semantic YAML patches. Scalar replacements use the surgical editor so
/// comments and layout remain untouched. Structural changes are serialized once
/// into serde_yaml's deterministic representation.
pub(crate) fn apply_patches(input: &str, patches: &[Patch]) -> Result<String> {
    let newline = line_ending(input);
    let mut output = input.to_owned();
    let mut structural = Vec::new();
    for patch in patches {
        match patch {
            Patch::Set(path, value) if scalar(value) => {
                let rendered = serde_yaml::to_string(value)?.trim().to_owned();
                match replace_scalar(&output, path, &rendered) {
                    Ok(updated) => output = updated,
                    Err(_) => structural.push(patch),
                }
            },
            Patch::Set(path, value) => match replace_structure(&output, path, value) {
                Ok(updated) => output = updated,
                Err(_) => structural.push(patch),
            },
            _ => structural.push(patch),
        }
    }
    if structural.is_empty() {
        return Ok(output);
    }
    let mut document: Value = serde_yaml::from_str(&output)?;
    for patch in structural {
        match patch {
            Patch::Set(path, value) => set_value(&mut document, path, value.clone())?,
            Patch::Remove(path) => remove_value(&mut document, path)?,
        }
    }
    let serialized = serde_yaml::to_string(&document).context("serialize patched YAML")?;
    Ok(with_line_ending(serialized, newline))
}

fn replace_structure(input: &str, path: &[Segment], value: &Value) -> Result<String> {
    let newline = line_ending(input);
    let trailing_newline = input.ends_with('\n');
    let mut lines = input.lines().map(str::to_owned).collect::<Vec<_>>();
    let (start, end, key) = locate_mapping_node(&lines, 0, lines.len(), None, path)?;
    let indent = indentation(&lines[start]);
    let padding = " ".repeat(indent);
    let child_padding = " ".repeat(indent + 2);
    let serialized = serde_yaml::to_string(value)?;
    let body = serialized.trim_end_matches('\n');
    let mut replacement = vec![format!("{padding}{key}:")];
    replacement.extend(body.lines().map(|line| format!("{child_padding}{line}")));
    lines.splice(start..end, replacement);
    let mut output = lines.join(newline);
    if trailing_newline {
        output.push_str(newline);
    }
    Ok(output)
}

fn locate_mapping_node(
    lines: &[String],
    start: usize,
    end: usize,
    parent_indent: Option<usize>,
    path: &[Segment],
) -> Result<(usize, usize, String)> {
    let (segment, remaining) = path.split_first().context("empty YAML path")?;
    let Segment::Key(key) = segment else {
        bail!("structural replacement requires mapping keys")
    };
    let index = find_key(lines, start, end, parent_indent, key)
        .with_context(|| format!("YAML key {key}"))?;
    let indent = indentation(&lines[index]);
    let node_end = block_end(lines, index + 1, end, indent);
    if remaining.is_empty() {
        return Ok((index, node_end, key.clone()));
    }
    locate_mapping_node(lines, index + 1, node_end, Some(indent), remaining)
}

fn scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn set_value(document: &mut Value, path: &[Segment], value: Value) -> Result<()> {
    let (last, parents) = path.split_last().context("empty YAML patch path")?;
    let parent = descend(document, parents, true)?;
    match (parent, last) {
        (Value::Mapping(mapping), Segment::Key(key)) => {
            mapping.insert(Value::String(key.clone()), value);
        },
        (Value::Sequence(sequence), Segment::Index(index)) if *index < sequence.len() => {
            sequence[*index] = value;
        },
        (Value::Sequence(sequence), Segment::Index(index)) if *index == sequence.len() => {
            sequence.push(value);
        },
        _ => bail!("YAML patch target has incompatible type"),
    }
    Ok(())
}

fn remove_value(document: &mut Value, path: &[Segment]) -> Result<()> {
    let (last, parents) = path.split_last().context("empty YAML patch path")?;
    let parent = descend(document, parents, false)?;
    match (parent, last) {
        (Value::Mapping(mapping), Segment::Key(key)) => {
            mapping.remove(Value::String(key.clone()));
        },
        (Value::Sequence(sequence), Segment::Index(index)) if *index < sequence.len() => {
            sequence.remove(*index);
        },
        _ => bail!("YAML removal target does not exist"),
    }
    Ok(())
}

fn descend<'a>(value: &'a mut Value, path: &[Segment], create: bool) -> Result<&'a mut Value> {
    let mut current = value;
    for segment in path {
        match segment {
            Segment::Key(key) => {
                if !matches!(current, Value::Mapping(_)) && create {
                    *current = Value::Mapping(Mapping::new());
                }
                let Value::Mapping(mapping) = current else {
                    bail!("YAML path component {key} is not a mapping")
                };
                let key = mapping_key(mapping, key);
                if create && !mapping.contains_key(&key) {
                    mapping.insert(key.clone(), Value::Mapping(Mapping::new()));
                }
                current = mapping.get_mut(&key).context("YAML path does not exist")?;
            },
            Segment::Index(index) => {
                let Value::Sequence(sequence) = current else {
                    bail!("YAML path component {index} is not a sequence")
                };
                current = sequence
                    .get_mut(*index)
                    .context("YAML index does not exist")?;
            },
        }
    }
    Ok(current)
}

fn mapping_key(mapping: &Mapping, key: &str) -> Value {
    let string = Value::String(key.into());
    if mapping.contains_key(&string) {
        return string;
    }
    key.parse::<u64>()
        .ok()
        .map(serde_yaml::Number::from)
        .map(Value::Number)
        .filter(|number| mapping.contains_key(number))
        .unwrap_or(string)
}

pub(crate) fn replace_scalar(input: &str, path: &[Segment], value: &str) -> Result<String> {
    let newline = line_ending(input);
    let trailing_newline = input.ends_with('\n');
    let mut lines = input.lines().map(str::to_owned).collect::<Vec<_>>();
    let end = lines.len();
    replace_in(&mut lines, 0, end, None, path, value)?;
    let mut output = lines.join(newline);
    if trailing_newline {
        output.push_str(newline);
    }
    Ok(output)
}

fn line_ending(input: &str) -> &'static str {
    if input.contains("\r\n") { "\r\n" } else { "\n" }
}

fn with_line_ending(value: String, newline: &str) -> String {
    if newline == "\r\n" {
        value.replace('\n', "\r\n")
    } else {
        value
    }
}

fn replace_in(
    lines: &mut [String],
    start: usize,
    end: usize,
    parent_indent: Option<usize>,
    path: &[Segment],
    value: &str,
) -> Result<()> {
    let (segment, remaining) = path.split_first().context("empty YAML path")?;
    match segment {
        Segment::Key(key) => {
            let index = find_key(lines, start, end, parent_indent, key)
                .with_context(|| format!("YAML key {key}"))?;
            if remaining.is_empty() {
                return replace_line_value(&mut lines[index], key, value);
            }
            let after = mapping_value(&lines[index], key)?;
            if after.trim_start().starts_with('{') {
                let [Segment::Key(child)] = remaining else {
                    bail!("unsupported nested flow mapping at {key}")
                };
                return replace_flow_value(&mut lines[index], child, value);
            }
            if !after.trim().is_empty() {
                bail!("YAML key {key} is not a mapping")
            }
            let indent = indentation(&lines[index]);
            let block_end = block_end(lines, index + 1, end, indent);
            replace_in(lines, index + 1, block_end, Some(indent), remaining, value)
        },
        Segment::Index(wanted) => {
            let index = find_sequence_item(lines, start, end, parent_indent, *wanted)
                .with_context(|| format!("YAML sequence item {wanted}"))?;
            let content = lines[index]
                .trim_start()
                .trim_start_matches('-')
                .trim_start();
            let [Segment::Key(child)] = remaining else {
                bail!("sequence adoption requires one child field")
            };
            if content.starts_with('{') {
                return replace_flow_value(&mut lines[index], child, value);
            }
            let indent = indentation(&lines[index]);
            let item_end = sequence_item_end(lines, index + 1, end, indent);
            replace_in(lines, index + 1, item_end, Some(indent), remaining, value)
        },
    }
}

fn find_key(
    lines: &[String],
    start: usize,
    end: usize,
    parent_indent: Option<usize>,
    key: &str,
) -> Option<usize> {
    let mut matches = (start..end)
        .filter(|index| parent_indent.is_none_or(|parent| indentation(&lines[*index]) > parent))
        .filter(|index| mapping_value(&lines[*index], key).is_ok())
        .collect::<Vec<_>>();
    let minimum = matches
        .iter()
        .map(|index| indentation(&lines[*index]))
        .min()?;
    matches.retain(|index| indentation(&lines[*index]) == minimum);
    (matches.len() == 1).then(|| matches[0])
}

fn find_sequence_item(
    lines: &[String],
    start: usize,
    end: usize,
    parent_indent: Option<usize>,
    wanted: usize,
) -> Option<usize> {
    let candidates = (start..end)
        .filter(|index| lines[*index].trim_start().starts_with("- "))
        .filter(|index| parent_indent.is_none_or(|parent| indentation(&lines[*index]) >= parent))
        .collect::<Vec<_>>();
    let minimum = candidates
        .iter()
        .map(|index| indentation(&lines[*index]))
        .min()?;
    candidates
        .into_iter()
        .filter(|index| indentation(&lines[*index]) == minimum)
        .nth(wanted)
}

fn mapping_value<'a>(line: &'a str, key: &str) -> Result<&'a str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('-') || trimmed.starts_with('#') {
        bail!("not a mapping entry")
    }
    let rest = trimmed.strip_prefix(key).context("different YAML key")?;
    rest.strip_prefix(':').context("YAML key boundary")
}

fn replace_line_value(line: &mut String, key: &str, value: &str) -> Result<()> {
    mapping_value(line, key)?;
    let colon = line.find(':').context("YAML mapping colon")?;
    let suffix = line[colon + 1..]
        .find(" #")
        .map(|offset| line[colon + 1 + offset..].to_owned())
        .unwrap_or_default();
    line.replace_range(colon + 1.., &format!(" {value}{suffix}"));
    Ok(())
}

fn replace_flow_value(line: &mut String, key: &str, value: &str) -> Result<()> {
    let open = line.find('{').context("flow mapping start")?;
    let close = line.rfind('}').context("flow mapping end")?;
    let body = &line[open + 1..close];
    let mut offset = 0;
    for part in body.split_inclusive(',') {
        let without_comma = part.strip_suffix(',').unwrap_or(part);
        let leading = without_comma.len() - without_comma.trim_start().len();
        let trimmed = without_comma.trim();
        if let Some(rest) = trimmed
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
        {
            let value_start = open + 1 + offset + leading + key.len() + 1;
            let whitespace = rest.len() - rest.trim_start().len();
            let value_end = open + 1 + offset + without_comma.len();
            line.replace_range(
                value_start..value_end,
                &format!("{}{value}", " ".repeat(whitespace)),
            );
            return Ok(());
        }
        offset += part.len();
    }
    bail!("flow mapping key {key} not found")
}

fn block_end(lines: &[String], start: usize, end: usize, parent_indent: usize) -> usize {
    (start..end)
        .find(|index| {
            !lines[*index].trim().is_empty() && indentation(&lines[*index]) <= parent_indent
        })
        .unwrap_or(end)
}

fn sequence_item_end(lines: &[String], start: usize, end: usize, item_indent: usize) -> usize {
    (start..end)
        .find(|index| {
            !lines[*index].trim().is_empty()
                && indentation(&lines[*index]) <= item_indent
                && lines[*index].trim_start().starts_with('-')
        })
        .unwrap_or(end)
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_inline_style_comments_and_unrelated_text() {
        let input = "# keep\nlxcs:\n  106:\n    rootfs: {storage: VMs, size_gb: 16}\n    start: {onboot: true, order: 4} # keep\n";
        let output = replace_scalar(
            input,
            &[
                Segment::Key("lxcs".into()),
                Segment::Key("106".into()),
                Segment::Key("rootfs".into()),
                Segment::Key("size_gb".into()),
            ],
            "12",
        )
        .unwrap();
        assert_eq!(
            output,
            "# keep\nlxcs:\n  106:\n    rootfs: {storage: VMs, size_gb: 12}\n    start: {onboot: true, order: 4} # keep\n"
        );
    }

    #[test]
    fn preserves_windows_line_endings() {
        let input = "lxcs:\r\n  101:\r\n    cores: 2 # keep\r\n";
        let output = replace_scalar(
            input,
            &[
                Segment::Key("lxcs".into()),
                Segment::Key("101".into()),
                Segment::Key("cores".into()),
            ],
            "4",
        )
        .unwrap();

        assert_eq!(output, "lxcs:\r\n  101:\r\n    cores: 4 # keep\r\n");
        assert!(!output.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn edits_one_sequence_item_without_reformatting() {
        let input = "lxcs:\n  106:\n    bind_mounts:\n      - {source: /a, target: /b, backed_up_by_pve: false}\n      - source: /c\n        target: /d\n        backed_up_by_pve: false # keep\n";
        let output = replace_scalar(
            input,
            &[
                Segment::Key("lxcs".into()),
                Segment::Key("106".into()),
                Segment::Key("bind_mounts".into()),
                Segment::Index(1),
                Segment::Key("backed_up_by_pve".into()),
            ],
            "true",
        )
        .unwrap();
        assert!(output.contains("backed_up_by_pve: false}\n"));
        assert!(output.contains("backed_up_by_pve: true # keep\n"));
    }

    #[test]
    fn structural_patch_updates_numeric_mapping_key() {
        let input = "lxcs:\n  105:\n    hostname: docker\n";
        let output = apply_patches(
            input,
            &[Patch::Set(
                vec![
                    Segment::Key("lxcs".into()),
                    Segment::Key("105".into()),
                    Segment::Key("firewall".into()),
                ],
                serde_yaml::from_str("{enabled: true, log_level_in: null, rules: []}").unwrap(),
            )],
        )
        .unwrap();
        let parsed: Value = serde_yaml::from_str(&output).unwrap();

        assert!(
            parsed["lxcs"][105]["firewall"]["enabled"]
                .as_bool()
                .unwrap()
        );
        assert!(parsed["lxcs"].get("105").is_none());
    }

    #[test]
    fn generic_patches_add_and_remove_collection_members() {
        let input = "jobs:\n  old: {schedule: daily}\nitems: [one, two]\n";
        let output = apply_patches(
            input,
            &[
                Patch::Remove(vec![
                    Segment::Key("jobs".into()),
                    Segment::Key("old".into()),
                ]),
                Patch::Set(
                    vec![Segment::Key("jobs".into()), Segment::Key("new".into())],
                    serde_yaml::from_str("{schedule: weekly}").unwrap(),
                ),
                Patch::Remove(vec![Segment::Key("items".into()), Segment::Index(0)]),
            ],
        )
        .unwrap();
        let value: Value = serde_yaml::from_str(&output).unwrap();
        assert!(value["jobs"].get("old").is_none());
        assert_eq!(value["jobs"]["new"]["schedule"], "weekly");
        assert_eq!(value["items"].as_sequence().unwrap().len(), 1);
    }
}
