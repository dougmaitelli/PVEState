mod bootstrap_pbs;
mod bootstrap_pve;
mod identity;
mod plan;
mod restore;

use crate::{
    client::RemoteHost,
    config::LocalState,
    settings::RecoverySettings,
    utility::{progress::EventSink, runtime_security},
};
use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Plan,
    BootstrapPve,
    BootstrapPbs,
    Restore,
    All,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum RecoveryReport {
    Plan {
        plan: plan::RecoveryPlan,
    },
    Completed {
        stage: String,
        message: Option<String>,
    },
}

pub(crate) fn run(
    repo: &LocalState,
    stage: Stage,
    target: &str,
    settings: &RecoverySettings,
    ssh: Option<&dyn RemoteHost>,
    events: &dyn EventSink,
) -> Result<RecoveryReport> {
    events.section(&format!("Recovery: {}", stage.name()));
    runtime_security::prepare(&repo.runtime())?;

    if matches!(stage, Stage::Plan) {
        return Ok(RecoveryReport::Plan {
            plan: plan::create(repo, target, events)?,
        });
    }

    let recovery_plan = plan::load(repo)?;
    plan::authorize(&recovery_plan, target, settings)?;
    let ssh = ssh.context("recovery SSH settings are required")?;
    identity::verify(ssh, &recovery_plan)?;

    match stage {
        Stage::BootstrapPve => bootstrap_pve::run(repo, ssh, events),
        Stage::BootstrapPbs => bootstrap_pbs::run(repo, ssh, events),
        Stage::Restore => restore::run(repo, ssh, events),
        Stage::All => {
            bootstrap_pve::run(repo, ssh, events)?;
            bootstrap_pbs::run(repo, ssh, events)?;
            restore::run(repo, ssh, events)
        },
        Stage::Plan => unreachable!(),
    }?;

    Ok(RecoveryReport::Completed {
        stage: stage.name().into(),
        message: matches!(stage, Stage::BootstrapPve | Stage::All).then(|| {
            "replacement PVE configuration staged; activate networking only with console access"
                .into()
        }),
    })
}

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::BootstrapPve => "bootstrap PVE",
            Self::BootstrapPbs => "bootstrap PBS",
            Self::Restore => "restore guests",
            Self::All => "all stages",
        }
    }
}
