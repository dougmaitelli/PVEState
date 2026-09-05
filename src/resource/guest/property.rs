use anyhow::{Result, bail};
use std::collections::BTreeMap;

pub(super) fn parse(value: &str) -> Result<BTreeMap<String, String>> {
    let parsed = crate::utility::property_string::parse(value)?;
    if parsed.positional.len() > 1 {
        bail!("guest property supports at most one positional value")
    }
    let mut options = parsed
        .keyed
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect::<BTreeMap<_, _>>();
    if let Some(volume) = parsed.positional.first() {
        options.insert("volume".into(), (*volume).into());
    }
    Ok(options)
}

pub(super) fn render(options: &BTreeMap<String, String>) -> String {
    let mut values = Vec::new();
    if let Some(volume) = options.get("volume") {
        values.push(volume.clone());
    }
    values.extend(
        options
            .iter()
            .filter(|(key, _)| key.as_str() != "volume")
            .map(|(key, value)| format!("{key}={value}")),
    );
    values.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_property_round_trip_preserves_unknown_keys() {
        let input = "local-lvm:vm-201-disk-0,discard=on,future=value,size=32G";
        let parsed = parse(input).unwrap();
        assert_eq!(parsed["future"], "value");
        assert_eq!(parse(&render(&parsed)).unwrap(), parsed);
    }
}
