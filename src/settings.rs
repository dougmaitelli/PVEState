use crate::reconcile::Domain;
use anyhow::{Context, Result, bail};
use reqwest::Url;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(crate) trait EnvSource {
    fn get(&self, name: &str) -> Option<OsString>;
}

struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

#[derive(Default)]
struct DotenvFile(BTreeMap<String, OsString>);

impl DotenvFile {
    fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let values = dotenvy::from_path_iter(path)
            .with_context(|| format!("read {}", path.display()))?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?
            .into_iter()
            .map(|(key, value)| (key, value.into()))
            .collect();
        Ok(Self(values))
    }
}

impl EnvSource for DotenvFile {
    fn get(&self, name: &str) -> Option<OsString> {
        self.0.get(name).cloned()
    }
}

struct LayeredEnv<'a> {
    primary: &'a dyn EnvSource,
    fallback: &'a dyn EnvSource,
}

impl EnvSource for LayeredEnv<'_> {
    fn get(&self, name: &str) -> Option<OsString> {
        self.primary.get(name).or_else(|| self.fallback.get(name))
    }
}

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
        let dotenv = DotenvFile::load(&root.join(crate::config::artifacts::ENV_FILE))?;
        let source = LayeredEnv {
            primary: &ProcessEnv,
            fallback: &dotenv,
        };
        Self::from_source(root, &source)
    }

    pub(crate) fn from_source(root: &Path, source: &dyn EnvSource) -> Result<Self> {
        let host = required(source, env::PVE_HOST)?;
        let api_scheme = value(source, env::PVE_API_SCHEME)?.unwrap_or_else(|| "https".into());
        if !matches!(api_scheme.as_str(), "http" | "https") {
            bail!("PVE_API_SCHEME must be http or https")
        }
        let api_port = port(source, env::PVE_API_PORT, 8006)?;
        let ssh_port = port(source, env::PVE_SSH_PORT, 22)?;
        let ssh_user = value(source, env::PVE_SSH_USER)?.unwrap_or_else(|| "root".into());
        let pve_endpoint = Url::parse(&format!("{api_scheme}://{host}:{api_port}"))
            .context("invalid PVE endpoint")?;
        let pbs_endpoint =
            Url::parse(&required(source, env::PBS_ENDPOINT)?).context("invalid PBS_ENDPOINT")?;

        let discovery_ssh = SshTarget {
            host: host.clone(),
            port: ssh_port,
            user: ssh_user.clone(),
            key: root.join(".secrets/pve_discovery"),
            known_hosts: root.join(".secrets/known_hosts"),
        };
        let mutation_ssh = optional_paths(source, env::APPLY_SSH_KEY, env::APPLY_KNOWN_HOSTS)?.map(
            |(key, known_hosts)| SshTarget {
                host: host.clone(),
                port: ssh_port,
                user: ssh_user,
                key,
                known_hosts,
            },
        );
        let recovery_port = port(source, env::TARGET_SSH_PORT, 22)?;
        let recovery_user = value(source, env::TARGET_SSH_USER)?.unwrap_or_else(|| "root".into());
        let recovery_ssh = optional_paths(source, env::TARGET_SSH_KEY, env::TARGET_KNOWN_HOSTS)?
            .map(|(key, known_hosts)| SshTargetTemplate {
                port: recovery_port,
                user: recovery_user,
                key,
                known_hosts,
            });

        let secrets = [env::PBS_APPLY_S3_ACCESS_KEY, env::PBS_APPLY_S3_SECRET_KEY]
            .into_iter()
            .filter_map(|name| {
                value(source, name)
                    .transpose()
                    .map(|value| value.map(|secret| (name.into(), secret)))
            })
            .collect::<Result<_>>()?;

        Ok(Self {
            pve: PveSettings {
                endpoint: pve_endpoint,
                verify_tls: boolean(source, env::PVE_VERIFY_TLS, true)?,
                ca_file: value(source, env::PVE_CA_FILE)?.map(PathBuf::from),
                discovery: credential(source, env::PVE_API_TOKEN_ID, env::PVE_API_TOKEN_SECRET)?,
                mutation: credential(
                    source,
                    env::PVE_APPLY_API_TOKEN_ID,
                    env::PVE_APPLY_API_TOKEN_SECRET,
                )?,
            },
            pbs: PbsSettings {
                endpoint: pbs_endpoint,
                verify_tls: boolean(source, env::PBS_VERIFY_TLS, true)?,
                ca_file: value(source, env::PBS_CA_FILE)?.map(PathBuf::from),
                discovery: credential(source, env::PBS_API_TOKEN_ID, env::PBS_API_TOKEN_SECRET)?,
                mutation: credential(
                    source,
                    env::PBS_APPLY_API_TOKEN_ID,
                    env::PBS_APPLY_API_TOKEN_SECRET,
                )?,
            },
            ssh: SshSettings {
                discovery: discovery_ssh,
                mutation: mutation_ssh,
                recovery: recovery_ssh,
            },
            apply: ApplySettings {
                enabled: yes(source, env::ENABLE_PRODUCTION_APPLY)?,
                confirm_plan_sha: value(source, env::CONFIRM_PLAN_SHA)?,
                target: value(source, env::APPLY_TARGET)?
                    .map(|target| Url::parse(&target).context("invalid PVES_APPLY_TARGET"))
                    .transpose()?,
                domains: value(source, env::APPLY_DOMAINS)?
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|domain| !domain.is_empty())
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .context("invalid PVES_APPLY_DOMAINS")?,
                activate_network: yes(source, env::APPLY_NETWORK_NOW)?,
                secrets,
            },
            recovery: RecoverySettings {
                enabled: yes(source, env::ENABLE_RECOVERY)?,
                confirm_plan_sha: value(source, env::CONFIRM_PLAN_SHA)?,
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

fn credential(source: &dyn EnvSource, id: &str, secret: &str) -> Result<Option<ApiCredential>> {
    match (value(source, id)?, value(source, secret)?) {
        (Some(id), Some(secret)) => Ok(Some(ApiCredential { id, secret })),
        (None, None) => Ok(None),
        _ => bail!("{id} and {secret} must be configured together"),
    }
}

fn optional_paths(
    source: &dyn EnvSource,
    key: &str,
    known: &str,
) -> Result<Option<(PathBuf, PathBuf)>> {
    match (value(source, key)?, value(source, known)?) {
        (Some(key), Some(known)) => Ok(Some((key.into(), known.into()))),
        (None, None) => Ok(None),
        _ => bail!("{key} and {known} must be configured together"),
    }
}

fn required(source: &dyn EnvSource, name: &str) -> Result<String> {
    value(source, name)?.with_context(|| format!("missing {name}"))
}

fn value(source: &dyn EnvSource, name: &str) -> Result<Option<String>> {
    let Some(value) = source.get(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    value
        .into_string()
        .map(Some)
        .map_err(|_| anyhow::anyhow!("{name} is not valid Unicode"))
}

fn port(source: &dyn EnvSource, name: &str, default: u16) -> Result<u16> {
    value(source, name)?
        .map(|value| value.parse().with_context(|| format!("invalid {name}")))
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn boolean(source: &dyn EnvSource, name: &str, default: bool) -> Result<bool> {
    match value(source, name)?.as_deref() {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => bail!("{name} must be true or false"),
    }
}

fn yes(source: &dyn EnvSource, name: &str) -> Result<bool> {
    Ok(value(source, name)?.as_deref() == Some("YES"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MapEnv(BTreeMap<String, OsString>);

    impl MapEnv {
        fn with(mut self, name: &str, value: impl Into<OsString>) -> Self {
            self.0.insert(name.into(), value.into());
            self
        }

        fn valid() -> Self {
            Self::default()
                .with(env::PVE_HOST, "pve.test")
                .with(env::PBS_ENDPOINT, "https://pbs.test:8007")
        }
    }

    impl EnvSource for MapEnv {
        fn get(&self, name: &str) -> Option<OsString> {
            self.0.get(name).cloned()
        }
    }

    #[test]
    fn missing_and_empty_required_values_are_rejected() {
        let root = Path::new("/configuration");
        assert!(Settings::from_source(root, &MapEnv::default()).is_err());
        let empty = MapEnv::valid().with(env::PVE_HOST, "");
        let error = Settings::from_source(root, &empty).err().unwrap();

        assert!(error.to_string().contains("missing PVE_HOST"));
    }

    #[test]
    fn malformed_typed_values_are_rejected_without_global_environment() {
        let root = Path::new("/configuration");
        for (name, value, expected) in [
            (env::PVE_API_PORT, "many", "invalid PVE_API_PORT"),
            (env::PVE_VERIFY_TLS, "yes", "must be true or false"),
            (env::APPLY_DOMAINS, "unknown", "invalid PVES_APPLY_DOMAINS"),
        ] {
            let source = MapEnv::valid().with(name, value);
            let error = Settings::from_source(root, &source).err().unwrap();

            assert!(format!("{error:#}").contains(expected));
        }
    }

    #[test]
    fn incomplete_secret_pairs_are_rejected() {
        let source = MapEnv::valid().with(env::PVE_API_TOKEN_ID, "operator@pve!capture");
        let error = Settings::from_source(Path::new("/configuration"), &source)
            .err()
            .unwrap();

        assert!(error.to_string().contains(env::PVE_API_TOKEN_ID));
        assert!(error.to_string().contains(env::PVE_API_TOKEN_SECRET));
    }

    #[test]
    fn process_layer_takes_precedence_over_dotenv_layer() {
        let process = MapEnv::valid().with(env::PVE_API_PORT, "9000");
        let dotenv = MapEnv::default().with(env::PVE_API_PORT, "8006");
        let layered = LayeredEnv {
            primary: &process,
            fallback: &dotenv,
        };

        let settings = Settings::from_source(Path::new("/configuration"), &layered).unwrap();

        assert_eq!(settings.pve.endpoint.as_str(), "https://pve.test:9000/");
    }

    #[test]
    fn unicode_values_and_secret_pairs_are_loaded_from_a_source() {
        let source = MapEnv::valid()
            .with(env::PVE_API_TOKEN_ID, "usuario@pve!captura")
            .with(env::PVE_API_TOKEN_SECRET, "sëcret");

        let settings = Settings::from_source(Path::new("/configuration"), &source).unwrap();

        let credential = settings.pve.discovery.unwrap();
        assert_eq!(credential.id, "usuario@pve!captura");
        assert_eq!(credential.secret, "sëcret");
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_values_return_a_named_error() {
        use std::os::unix::ffi::OsStringExt;

        let source = MapEnv::valid().with(env::PVE_HOST, OsString::from_vec(vec![0xff]));
        let error = Settings::from_source(Path::new("/configuration"), &source)
            .err()
            .unwrap();

        assert_eq!(error.to_string(), "PVE_HOST is not valid Unicode");
    }

    #[test]
    fn dotenv_parser_does_not_modify_process_environment() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.env");
        std::fs::write(
            &path,
            "PVE_HOST=from-file\nPBS_ENDPOINT=https://pbs.file:8007\n",
        )
        .unwrap();

        let dotenv = DotenvFile::load(&path).unwrap();

        assert_eq!(dotenv.get(env::PVE_HOST), Some("from-file".into()));
        assert_eq!(
            dotenv.get(env::PBS_ENDPOINT),
            Some("https://pbs.file:8007".into())
        );
    }
}
