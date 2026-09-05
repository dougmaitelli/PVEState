use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct QemuAgentOptions {
    pub(crate) enabled: bool,
    pub(crate) fstrim_cloned_disks: Option<bool>,
    pub(crate) freeze_fs_on_backup: Option<bool>,
    pub(crate) passthrough: BTreeMap<String, String>,
}

impl QemuAgentOptions {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let property = crate::utility::property_string::parse(value)?;
        if property.positional.len() != 1 {
            bail!("QEMU agent requires exactly one positional enabled value")
        }
        let enabled =
            parse_bool(property.positional[0]).context("invalid QEMU agent enabled value")?;
        let mut result = Self {
            enabled,
            ..Self::default()
        };
        for (key, value) in property.keyed {
            match key {
                "fstrim_cloned_disks" => {
                    result.fstrim_cloned_disks =
                        Some(parse_bool(value).with_context(|| format!("invalid `{key}` value"))?);
                },
                "freeze-fs-on-backup" => {
                    result.freeze_fs_on_backup =
                        Some(parse_bool(value).with_context(|| format!("invalid `{key}` value"))?);
                },
                _ => {
                    result.passthrough.insert(key.to_owned(), value.to_owned());
                },
            }
        }
        Ok(result)
    }

    pub(crate) fn from_api(value: Option<&Value>) -> Result<Self> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        if let Some(enabled) = value.as_bool() {
            return Ok(Self {
                enabled,
                ..Self::default()
            });
        }
        if let Some(enabled) = value.as_u64() {
            return match enabled {
                0 | 1 => Ok(Self {
                    enabled: enabled == 1,
                    ..Self::default()
                }),
                _ => bail!("invalid numeric QEMU agent value `{enabled}`"),
            };
        }
        Self::parse(
            value
                .as_str()
                .context("QEMU agent value is not a string or boolean")?,
        )
    }

    pub(crate) fn render(&self) -> String {
        let mut values = vec![u8::from(self.enabled).to_string()];
        if let Some(value) = self.fstrim_cloned_disks {
            values.push(format!("fstrim_cloned_disks={}", u8::from(value)));
        }
        if let Some(value) = self.freeze_fs_on_backup {
            values.push(format!("freeze-fs-on-backup={}", u8::from(value)));
        }
        values.extend(
            self.passthrough
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
        );
        values.join(",")
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "on" => Ok(true),
        "0" | "false" | "off" => Ok(false),
        _ => bail!("expected 0, 1, false, or true; found `{value}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_enabled_and_disabled_values() {
        assert!(QemuAgentOptions::parse("1").unwrap().enabled);
        assert!(!QemuAgentOptions::parse("0").unwrap().enabled);
    }

    #[test]
    fn parses_and_renders_known_and_passthrough_options() {
        let parsed =
            QemuAgentOptions::parse("1,fstrim_cloned_disks=1,freeze-fs-on-backup=0,type=virtio")
                .unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.fstrim_cloned_disks, Some(true));
        assert_eq!(parsed.freeze_fs_on_backup, Some(false));
        assert_eq!(parsed.passthrough["type"], "virtio");
        assert_eq!(
            parsed.render(),
            "1,fstrim_cloned_disks=1,freeze-fs-on-backup=0,type=virtio"
        );
    }

    #[test]
    fn rejects_malformed_and_duplicate_options() {
        assert!(QemuAgentOptions::parse("").is_err());
        assert!(QemuAgentOptions::parse("enabled").is_err());
        assert!(QemuAgentOptions::parse("1,broken").is_err());
        assert!(QemuAgentOptions::parse("1,fstrim_cloned_disks=maybe").is_err());
        assert!(QemuAgentOptions::parse("1,type=a,type=b").is_err());
    }
}
