use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AddressMethod {
    Manual,
    Static,
    Dhcp,
}

impl fmt::Display for AddressMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Manual => "manual",
            Self::Static => "static",
            Self::Dhcp => "dhcp",
        })
    }
}
impl AddressMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Static => "static",
            Self::Dhcp => "dhcp",
        }
    }
}
impl FromStr for AddressMethod {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "manual" => Ok(Self::Manual),
            "static" => Ok(Self::Static),
            "dhcp" => Ok(Self::Dhcp),
            _ => anyhow::bail!("unsupported address method {value}"),
        }
    }
}
impl PartialEq<str> for AddressMethod {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl PartialEq<&str> for AddressMethod {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Network {
    pub(crate) host: String,
    pub(crate) management_address: String,
    pub(crate) management_gateway: String,
    pub(crate) dns: Dns,
    pub(crate) interfaces: Vec<Interface>,
    pub(crate) bridges: Vec<Bridge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Dns {
    pub(crate) search: String,
    #[serde(default)]
    pub(crate) servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Interface {
    pub(crate) name: String,
    pub(crate) method: AddressMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bridge {
    pub(crate) name: String,
    pub(crate) method: AddressMethod,
    pub(crate) address: Option<String>,
    pub(crate) gateway: Option<String>,
    #[serde(default)]
    pub(crate) ipv6: Option<String>,
    #[serde(default)]
    pub(crate) gateway6: Option<String>,
    #[serde(default)]
    pub(crate) ports: Vec<String>,
    pub(crate) stp: bool,
    pub(crate) forward_delay: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_address_methods() {
        assert!(serde_yaml::from_str::<AddressMethod>("dynamic").is_err());
    }
}
