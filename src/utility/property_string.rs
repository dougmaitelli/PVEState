use anyhow::{Result, bail};
use std::collections::BTreeMap;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PropertyString<'a> {
    pub(crate) positional: Vec<&'a str>,
    pub(crate) keyed: BTreeMap<&'a str, &'a str>,
}

pub(crate) fn parse(input: &str) -> Result<PropertyString<'_>> {
    if input.is_empty() {
        return Ok(PropertyString::default());
    }
    let mut parsed = PropertyString::default();
    for item in input.split(',') {
        if item.is_empty() {
            bail!("property string contains an empty segment")
        }
        if let Some((key, value)) = item.split_once('=') {
            if key.is_empty() || value.is_empty() {
                bail!("property string contains malformed property `{item}`")
            }
            if parsed.keyed.insert(key, value).is_some() {
                bail!("property string contains duplicate key `{key}`")
            }
        } else {
            parsed.positional.push(item);
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_positional_keyed_and_unknown_values() {
        let parsed = parse("volume,discard=on,future=value").unwrap();
        assert_eq!(parsed.positional, ["volume"]);
        assert_eq!(parsed.keyed["discard"], "on");
        assert_eq!(parsed.keyed["future"], "value");
    }

    #[test]
    fn rejects_duplicates_empty_values_and_malformed_separators() {
        for input in ["a=1,a=2", "a=", "=value", "a=1,,b=2", ","] {
            assert!(parse(input).is_err(), "accepted {input}");
        }
    }
}
