use crate::command::plan::Domain;
use anyhow::{Context, Result, bail};
use reqwest::Url;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct Settings {
    pub(crate) pve: PveSettings,
    pub(crate) pbs: PbsSettings,
    pub(crate) ssh: SshSettings,
    pub(crate) apply: ApplySettings,
    pub(crate) recovery: RecoverySettings,
}

#[derive(Clone)]
pub(crate) struct PveSettings {
    pub(crate) endpoint: Url,
    pub(crate) verify_tls: bool,
    pub(crate) ca_file: Option<PathBuf>,
    pub(crate) discovery: Option<ApiCredential>,
    pub(crate) mutation: Option<ApiCredential>,
}

#[derive(Clone)]
pub(crate) struct PbsSettings {
    pub(crate) endpoint: Url,
    pub(crate) verify_tls: bool,
    pub(crate) ca_file: Option<PathBuf>,
    pub(crate) discovery: Option<ApiCredential>,
    pub(crate) mutation: Option<ApiCredential>,
}

#[derive(Clone)]
pub(crate) struct ApiCredential {
    pub(crate) id: String,
    pub(crate) secret: String,
}

#[derive(Clone)]
pub(crate) struct SshSettings {
    pub(crate) discovery: SshTarget,
    pub(crate) mutation: Option<SshTarget>,
    pub(crate) recovery: Option<SshTargetTemplate>,
}

#[derive(Clone)]
pub(crate) struct SshTarget {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) key: PathBuf,
    pub(crate) known_hosts: PathBuf,
}

#[derive(Clone)]
pub(crate) struct SshTargetTemplate {
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) key: PathBuf,
    pub(crate) known_hosts: PathBuf,
}

#[derive(Clone)]
pub(crate) struct ApplySettings {
    pub(crate) enabled: bool,
    pub(crate) confirm_plan_sha: Option<String>,
    pub(crate) target: Option<Url>,
    pub(crate) domains: BTreeSet<Domain>,
    pub(crate) activate_network: bool,
    pub(crate) secrets: BTreeMap<String, String>,
}

#[derive(Clone)]
pub(crate) struct RecoverySettings {
    pub(crate) enabled: bool,
    pub(crate) confirm_plan_sha: Option<String>,
}

impl Settings {
    pub(crate) fn load(root: &Path) -> Result<Self> {
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
    pub(crate) fn discovery_credential(&self) -> Result<&ApiCredential> {
        self.discovery
            .as_ref()
            .context("missing PVE discovery API credential")
    }

    pub(crate) fn mutation_credential(&self) -> Result<&ApiCredential> {
        self.mutation
            .as_ref()
            .context("missing PVE mutation API credential")
    }
}

impl PbsSettings {
    pub(crate) fn discovery_credential(&self) -> Result<&ApiCredential> {
        self.discovery
            .as_ref()
            .context("missing PBS discovery API credential")
    }

    pub(crate) fn mutation_credential(&self) -> Result<&ApiCredential> {
        self.mutation
            .as_ref()
            .context("missing PBS mutation API credential")
    }
}

impl ApplySettings {
    pub(crate) fn secret(&self, name: &str) -> Result<&str> {
        self.secrets
            .get(name)
            .map(String::as_str)
            .with_context(|| format!("missing {name}"))
    }
}

impl SshTargetTemplate {
    pub(crate) fn for_host(&self, host: &str) -> SshTarget {
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
