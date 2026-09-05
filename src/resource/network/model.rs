use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AddressMethod {
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
pub struct Network {
    pub host: String,
    pub management_address: String,
    pub management_gateway: String,
    pub dns: Dns,
    pub interfaces: Vec<Interface>,
    pub bridges: Vec<Bridge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dns {
    pub search: String,
    #[serde(default)]
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    pub method: AddressMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    pub name: String,
    pub method: AddressMethod,
    pub address: Option<String>,
    pub gateway: Option<String>,
    #[serde(default)]
    pub ipv6: Option<String>,
    #[serde(default)]
    pub gateway6: Option<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    pub stp: bool,
    pub forward_delay: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_address_methods() {
        assert!(serde_yaml::from_str::<AddressMethod>("dynamic").is_err());
    }
}
