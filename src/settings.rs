use crate::command::plan::Domain;
use anyhow::{Context, Result, bail};
use reqwest::Url;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub(crate) mod env {
    pub(crate) const PVE_HOST: &str = "PVE_HOST";
    pub(crate) const PVE_API_SCHEME: &str = "PVE_API_SCHEME";
    pub(crate) const PVE_API_PORT: &str = "PVE_API_PORT";
    pub(crate) const PVE_SSH_PORT: &str = "PVE_SSH_PORT";
    pub(crate) const PVE_SSH_USER: &str = "PVE_SSH_USER";
    pub(crate) const PVE_VERIFY_TLS: &str = "PVE_VERIFY_TLS";
    pub(crate) const PVE_CA_FILE: &str = "PVE_CA_FILE";
    pub(crate) const PVE_API_TOKEN_ID: &str = "PVE_API_TOKEN_ID";
    pub(crate) const PVE_API_TOKEN_SECRET: &str = "PVE_API_TOKEN_SECRET";
    pub(crate) const PVE_APPLY_API_TOKEN_ID: &str = "PVE_APPLY_API_TOKEN_ID";
    pub(crate) const PVE_APPLY_API_TOKEN_SECRET: &str = "PVE_APPLY_API_TOKEN_SECRET";
    pub(crate) const PBS_ENDPOINT: &str = "PBS_ENDPOINT";
    pub(crate) const PBS_VERIFY_TLS: &str = "PBS_VERIFY_TLS";
    pub(crate) const PBS_CA_FILE: &str = "PBS_CA_FILE";
    pub(crate) const PBS_API_TOKEN_ID: &str = "PBS_API_TOKEN_ID";
    pub(crate) const PBS_API_TOKEN_SECRET: &str = "PBS_API_TOKEN_SECRET";
    pub(crate) const PBS_APPLY_API_TOKEN_ID: &str = "PBS_APPLY_API_TOKEN_ID";
    pub(crate) const PBS_APPLY_API_TOKEN_SECRET: &str = "PBS_APPLY_API_TOKEN_SECRET";
    pub(crate) const PBS_APPLY_S3_ACCESS_KEY: &str = "PBS_APPLY_S3_ACCESS_KEY";
    pub(crate) const PBS_APPLY_S3_SECRET_KEY: &str = "PBS_APPLY_S3_SECRET_KEY";
    pub(crate) const APPLY_SSH_KEY: &str = "PVES_APPLY_SSH_KEY";
    pub(crate) const APPLY_KNOWN_HOSTS: &str = "PVES_APPLY_KNOWN_HOSTS";
    pub(crate) const TARGET_SSH_PORT: &str = "PVES_TARGET_SSH_PORT";
    pub(crate) const TARGET_SSH_KEY: &str = "PVES_TARGET_SSH_KEY";
    pub(crate) const TARGET_KNOWN_HOSTS: &str = "PVES_TARGET_KNOWN_HOSTS";
    pub(crate) const TARGET_SSH_USER: &str = "PVES_TARGET_SSH_USER";
    pub(crate) const ENABLE_PRODUCTION_APPLY: &str = "PVES_ENABLE_PRODUCTION_APPLY";
    pub(crate) const ENABLE_RECOVERY: &str = "PVES_ENABLE_RECOVERY";
    pub(crate) const CONFIRM_PLAN_SHA: &str = "PVES_CONFIRM_PLAN_SHA";
    pub(crate) const APPLY_TARGET: &str = "PVES_APPLY_TARGET";
    pub(crate) const APPLY_DOMAINS: &str = "PVES_APPLY_DOMAINS";
    pub(crate) const APPLY_NETWORK_NOW: &str = "PVES_APPLY_NETWORK_NOW";
    pub(crate) const ENABLE_PRODUCTION_APPLY_ERROR: &str =
        "PVES_ENABLE_PRODUCTION_APPLY must equal YES";
    pub(crate) const ENABLE_RECOVERY_ERROR: &str = "PVES_ENABLE_RECOVERY must equal YES";
}

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
        dotenvy::from_path(root.join(crate::config::artifacts::ENV_FILE)).ok();
        let host = required(env::PVE_HOST)?;
        let api_scheme = value(env::PVE_API_SCHEME).unwrap_or_else(|| "https".into());
        if !matches!(api_scheme.as_str(), "http" | "https") {
            bail!("PVE_API_SCHEME must be http or https")
        }
        let api_port = port(env::PVE_API_PORT, 8006)?;
        let ssh_port = port(env::PVE_SSH_PORT, 22)?;
        let ssh_user = value(env::PVE_SSH_USER).unwrap_or_else(|| "root".into());
        let pve_endpoint = Url::parse(&format!("{api_scheme}://{host}:{api_port}"))
            .context("invalid PVE endpoint")?;
        let pbs_endpoint =
            Url::parse(&required(env::PBS_ENDPOINT)?).context("invalid PBS_ENDPOINT")?;

        let discovery_ssh = SshTarget {
            host: host.clone(),
            port: ssh_port,
            user: ssh_user.clone(),
            key: root.join(".secrets/pve_discovery"),
            known_hosts: root.join(".secrets/known_hosts"),
        };
        let mutation_ssh = optional_paths(env::APPLY_SSH_KEY, env::APPLY_KNOWN_HOSTS)?.map(
            |(key, known_hosts)| SshTarget {
                host: host.clone(),
                port: ssh_port,
                user: ssh_user,
                key,
                known_hosts,
            },
        );
        let recovery_port = port(env::TARGET_SSH_PORT, 22)?;
        let recovery_ssh = optional_paths(env::TARGET_SSH_KEY, env::TARGET_KNOWN_HOSTS)?.map(
            |(key, known_hosts)| SshTargetTemplate {
                port: recovery_port,
                user: value(env::TARGET_SSH_USER).unwrap_or_else(|| "root".into()),
                key,
                known_hosts,
            },
        );

        let secrets = [env::PBS_APPLY_S3_ACCESS_KEY, env::PBS_APPLY_S3_SECRET_KEY]
            .into_iter()
            .filter_map(|name| value(name).map(|secret| (name.into(), secret)))
            .collect();

        Ok(Self {
            pve: PveSettings {
                endpoint: pve_endpoint,
                verify_tls: boolean(env::PVE_VERIFY_TLS, true)?,
                ca_file: value(env::PVE_CA_FILE).map(PathBuf::from),
                discovery: credential(env::PVE_API_TOKEN_ID, env::PVE_API_TOKEN_SECRET)?,
                mutation: credential(env::PVE_APPLY_API_TOKEN_ID, env::PVE_APPLY_API_TOKEN_SECRET)?,
            },
            pbs: PbsSettings {
                endpoint: pbs_endpoint,
                verify_tls: boolean(env::PBS_VERIFY_TLS, true)?,
                ca_file: value(env::PBS_CA_FILE).map(PathBuf::from),
                discovery: credential(env::PBS_API_TOKEN_ID, env::PBS_API_TOKEN_SECRET)?,
                mutation: credential(env::PBS_APPLY_API_TOKEN_ID, env::PBS_APPLY_API_TOKEN_SECRET)?,
            },
            ssh: SshSettings {
                discovery: discovery_ssh,
                mutation: mutation_ssh,
                recovery: recovery_ssh,
            },
            apply: ApplySettings {
                enabled: yes(env::ENABLE_PRODUCTION_APPLY),
                confirm_plan_sha: value(env::CONFIRM_PLAN_SHA),
                target: value(env::APPLY_TARGET)
                    .map(|target| Url::parse(&target).context("invalid PVES_APPLY_TARGET"))
                    .transpose()?,
                domains: value(env::APPLY_DOMAINS)
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|domain| !domain.is_empty())
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .context("invalid PVES_APPLY_DOMAINS")?,
                activate_network: yes(env::APPLY_NETWORK_NOW),
                secrets,
            },
            recovery: RecoverySettings {
                enabled: yes(env::ENABLE_RECOVERY),
                confirm_plan_sha: value(env::CONFIRM_PLAN_SHA),
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
