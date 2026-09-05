use crate::command::plan::Domain;
use anyhow::{Context, Result, bail};
use reqwest::Url;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Settings {
    pub pve: PveSettings,
    pub pbs: PbsSettings,
    pub ssh: SshSettings,
    pub apply: ApplySettings,
    pub recovery: RecoverySettings,
}

#[derive(Clone)]
pub struct PveSettings {
    pub endpoint: Url,
    pub verify_tls: bool,
    pub ca_file: Option<PathBuf>,
    pub discovery: Option<ApiCredential>,
    pub mutation: Option<ApiCredential>,
}

#[derive(Clone)]
pub struct PbsSettings {
    pub endpoint: Url,
    pub verify_tls: bool,
    pub ca_file: Option<PathBuf>,
    pub discovery: Option<ApiCredential>,
    pub mutation: Option<ApiCredential>,
}

#[derive(Clone)]
pub struct ApiCredential {
    pub id: String,
    pub secret: String,
}

#[derive(Clone)]
pub struct SshSettings {
    pub discovery: SshTarget,
    pub mutation: Option<SshTarget>,
    pub recovery: Option<SshTargetTemplate>,
}

#[derive(Clone)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key: PathBuf,
    pub known_hosts: PathBuf,
}

#[derive(Clone)]
pub struct SshTargetTemplate {
    pub port: u16,
    pub user: String,
    pub key: PathBuf,
    pub known_hosts: PathBuf,
}

#[derive(Clone)]
pub struct ApplySettings {
    pub enabled: bool,
    pub confirm_plan_sha: Option<String>,
    pub target: Option<Url>,
    pub domains: BTreeSet<Domain>,
    pub activate_network: bool,
    pub(crate) secrets: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct RecoverySettings {
    pub enabled: bool,
    pub confirm_plan_sha: Option<String>,
}

impl Settings {
    pub fn load(root: &Path) -> Result<Self> {
        dotenvy::from_path(root.join(".pves.env")).ok();
        let host = required("PVE_HOST")?;
        let api_scheme = value("PVE_API_SCHEME").unwrap_or_else(|| "https".into());
        if !matches!(api_scheme.as_str(), "http" | "https") {
            bail!("PVE_API_SCHEME must be http or https")
        }
        let api_port = port("PVE_API_PORT", 8006)?;
        let ssh_port = port("PVE_SSH_PORT", 22)?;
        let ssh_user = value("PVE_SSH_USER").unwrap_or_else(|| "root".into());
        let pve_endpoint = Url::parse(&format!("{api_scheme}://{host}:{api_port}"))
            .context("invalid PVE endpoint")?;
        let pbs_endpoint =
            Url::parse(&required("PBS_ENDPOINT")?).context("invalid PBS_ENDPOINT")?;

        let discovery_ssh = SshTarget {
            host: host.clone(),
            port: ssh_port,
            user: ssh_user.clone(),
            key: root.join(".secrets/pve_discovery"),
            known_hosts: root.join(".secrets/known_hosts"),
        };
        let mutation_ssh = optional_paths("PVES_APPLY_SSH_KEY", "PVES_APPLY_KNOWN_HOSTS")?.map(
            |(key, known_hosts)| SshTarget {
                host: host.clone(),
                port: ssh_port,
                user: ssh_user,
                key,
                known_hosts,
            },
        );
        let recovery_port = port("PVES_TARGET_SSH_PORT", 22)?;
        let recovery_ssh = optional_paths("PVES_TARGET_SSH_KEY", "PVES_TARGET_KNOWN_HOSTS")?.map(
            |(key, known_hosts)| SshTargetTemplate {
                port: recovery_port,
                user: value("PVES_TARGET_SSH_USER").unwrap_or_else(|| "root".into()),
                key,
                known_hosts,
            },
        );

        let secrets = ["PBS_APPLY_S3_ACCESS_KEY", "PBS_APPLY_S3_SECRET_KEY"]
            .into_iter()
            .filter_map(|name| value(name).map(|secret| (name.into(), secret)))
            .collect();

        Ok(Self {
            pve: PveSettings {
                endpoint: pve_endpoint,
                verify_tls: boolean("PVE_VERIFY_TLS", true)?,
                ca_file: value("PVE_CA_FILE").map(PathBuf::from),
                discovery: credential("PVE_API_TOKEN_ID", "PVE_API_TOKEN_SECRET")?,
                mutation: credential("PVE_APPLY_API_TOKEN_ID", "PVE_APPLY_API_TOKEN_SECRET")?,
            },
            pbs: PbsSettings {
                endpoint: pbs_endpoint,
                verify_tls: boolean("PBS_VERIFY_TLS", true)?,
                ca_file: value("PBS_CA_FILE").map(PathBuf::from),
                discovery: credential("PBS_API_TOKEN_ID", "PBS_API_TOKEN_SECRET")?,
                mutation: credential("PBS_APPLY_API_TOKEN_ID", "PBS_APPLY_API_TOKEN_SECRET")?,
            },
            ssh: SshSettings {
                discovery: discovery_ssh,
                mutation: mutation_ssh,
                recovery: recovery_ssh,
            },
            apply: ApplySettings {
                enabled: yes("PVES_ENABLE_PRODUCTION_APPLY"),
                confirm_plan_sha: value("PVES_CONFIRM_PLAN_SHA"),
                target: value("PVES_APPLY_TARGET")
                    .map(|target| Url::parse(&target).context("invalid PVES_APPLY_TARGET"))
                    .transpose()?,
                domains: value("PVES_APPLY_DOMAINS")
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|domain| !domain.is_empty())
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .context("invalid PVES_APPLY_DOMAINS")?,
                activate_network: yes("PVES_APPLY_NETWORK_NOW"),
                secrets,
            },
            recovery: RecoverySettings {
                enabled: yes("PVES_ENABLE_RECOVERY"),
                confirm_plan_sha: value("PVES_CONFIRM_PLAN_SHA"),
            },
        })
    }
}

impl PveSettings {
    pub fn discovery_credential(&self) -> Result<&ApiCredential> {
        self.discovery
            .as_ref()
            .context("missing PVE discovery API credential")
    }

    pub fn mutation_credential(&self) -> Result<&ApiCredential> {
        self.mutation
            .as_ref()
            .context("missing PVE mutation API credential")
    }
}

impl PbsSettings {
    pub fn discovery_credential(&self) -> Result<&ApiCredential> {
        self.discovery
            .as_ref()
            .context("missing PBS discovery API credential")
    }

    pub fn mutation_credential(&self) -> Result<&ApiCredential> {
        self.mutation
            .as_ref()
            .context("missing PBS mutation API credential")
    }
}

impl ApplySettings {
    pub fn secret(&self, name: &str) -> Result<&str> {
        self.secrets
            .get(name)
            .map(String::as_str)
            .with_context(|| format!("missing {name}"))
    }
}

impl SshTargetTemplate {
    pub fn for_host(&self, host: &str) -> SshTarget {
        SshTarget {
            host: host.into(),
            port: self.port,
            user: self.user.clone(),
            key: self.key.clone(),
            known_hosts: self.known_hosts.clone(),
        }
    }
}

fn credential(id: &str, secret: &str) -> Result<Option<ApiCredential>> {
    match (value(id), value(secret)) {
        (Some(id), Some(secret)) => Ok(Some(ApiCredential { id, secret })),
        (None, None) => Ok(None),
        _ => bail!("{id} and {secret} must be configured together"),
    }
}

fn optional_paths(key: &str, known: &str) -> Result<Option<(PathBuf, PathBuf)>> {
    match (value(key), value(known)) {
        (Some(key), Some(known)) => Ok(Some((key.into(), known.into()))),
        (None, None) => Ok(None),
        _ => bail!("{key} and {known} must be configured together"),
    }
}

fn required(name: &str) -> Result<String> {
    value(name).with_context(|| format!("missing {name}"))
}

fn value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn port(name: &str, default: u16) -> Result<u16> {
    value(name)
        .map(|value| value.parse().with_context(|| format!("invalid {name}")))
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn boolean(name: &str, default: bool) -> Result<bool> {
    match value(name).as_deref() {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => bail!("{name} must be true or false"),
    }
}

fn yes(name: &str) -> bool {
    value(name).as_deref() == Some("YES")
}
