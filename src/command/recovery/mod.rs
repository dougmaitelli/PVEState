mod bootstrap_pbs;
mod bootstrap_pve;
mod configure;
mod identity;
mod plan;
mod restore;

use crate::{
    client::RemoteHost,
    config::LocalState,
    settings::RecoverySettings,
    utility::{progress, runtime_security},
};
use anyhow::{Context, Result};

#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Plan,
    BootstrapPve,
    BootstrapPbs,
    Restore,
    Configure,
    All,
}

pub(crate) fn run(
    repo: &LocalState,
    stage: Stage,
    target: &str,
    settings: &RecoverySettings,
    ssh: Option<&dyn RemoteHost>,
) -> Result<()> {
    progress::section(format!("Recovery: {}", stage.name()));
    runtime_security::prepare(&repo.runtime())?;

    if matches!(stage, Stage::Plan) {
        return plan::create(repo, target);
    }

    let recovery_plan = plan::load(repo)?;
    plan::authorize(&recovery_plan, target, settings)?;
    let ssh = ssh.context("recovery SSH settings are required")?;
    identity::verify(ssh, &recovery_plan)?;

    match stage {
        Stage::BootstrapPve => bootstrap_pve::run(repo, ssh),
        Stage::BootstrapPbs => bootstrap_pbs::run(repo, ssh),
        Stage::Restore => restore::run(repo, ssh),
        Stage::Configure => configure::run(repo, ssh),
        Stage::All => {
            bootstrap_pve::run(repo, ssh)?;
            bootstrap_pbs::run(repo, ssh)?;
            restore::run(repo, ssh)?;
            configure::run(repo, ssh)
        },
        Stage::Plan => unreachable!(),
    }
}

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::BootstrapPve => "bootstrap PVE",
            Self::BootstrapPbs => "bootstrap PBS",
            Self::Restore => "restore guests",
            Self::Configure => "configure applications",
            Self::All => "all stages",
        }
    }
}
