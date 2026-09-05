use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, str::FromStr};

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
        pub enum $name { $(#[serde(rename=$value)] $variant),+ }
        impl fmt::Display for $name { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { f.write_str(match self {$(Self::$variant=>$value),+}) } }
        impl FromStr for $name { type Err=anyhow::Error; fn from_str(value:&str)->Result<Self,Self::Err>{match value{$($value=>Ok(Self::$variant)),+,_=>anyhow::bail!("unsupported {} `{value}`",stringify!($name))}} }
    };
}

string_enum!(FirewallDirection { In => "IN", Out => "OUT", Group => "GROUP" });
string_enum!(FirewallAction { Accept => "ACCEPT", Drop => "DROP", Reject => "REJECT", Return => "RETURN" });
string_enum!(FirewallProtocol { Tcp => "tcp", Udp => "udp", Icmp => "icmp", Icmpv6 => "icmpv6", Esp => "esp", Ah => "ah" });
string_enum!(FirewallLogLevel { NoLog => "nolog", Emergency => "emerg", Alert => "alert", Critical => "crit", Error => "err", Warning => "warning", Notice => "notice", Info => "info", Debug => "debug" });

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallPolicy {
    pub enabled: bool,
    #[serde(default)]
    pub log_level_in: Option<FirewallLogLevel>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<FirewallAlias>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_sets: Vec<FirewallIpSet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security_groups: Vec<FirewallSecurityGroup>,
    #[serde(default)]
    pub rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallAlias {
    pub name: String,
    pub network: String,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallIpSet {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<FirewallIpSetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallIpSetEntry {
    pub network: String,
    #[serde(default)]
    pub nomatch: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallSecurityGroup {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<FirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirewallRule {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub direction: FirewallDirection,
    pub action: FirewallAction,
    #[serde(default)]
    pub macro_name: Option<String>,
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub protocol: Option<FirewallProtocol>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub source_port: Option<String>,
    #[serde(default)]
    pub destination_port: Option<String>,
    #[serde(default = "no_log")]
    pub log: FirewallLogLevel,
    #[serde(default)]
    pub comment: Option<String>,
}

fn yes() -> bool {
    true
}

fn no_log() -> FirewallLogLevel {
    FirewallLogLevel::NoLog
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_firewall_vocabulary() {
        assert!(serde_yaml::from_str::<FirewallDirection>("SIDEWAYS").is_err());
        assert!(serde_yaml::from_str::<FirewallAction>("ALLOW").is_err());
        assert!(serde_yaml::from_str::<FirewallProtocol>("made-up").is_err());
        assert!(serde_yaml::from_str::<FirewallLogLevel>("verbose").is_err());
    }
}
